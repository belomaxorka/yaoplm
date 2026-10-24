//! Loaders through `dispatch` (LDR-1, LDR-2): the catalogue, what a download
//! would be, downloads held to the hash the user saw, the user's own files
//! and the catalogue from `main`. Releases come from a mirror folder with
//! GitHub's layout: no test goes out (CI sets `YAOPLM_OFFLINE`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobInfo, JobResult, JobStatus};
use yaoplm_api::loaders::{LoaderCatalogSource, LoaderChecksumSource, LoaderFile, LoaderOffer};
use yaoplm_api::{Api, JobId};
use yaoplm_test_support::loaders::{ReleaseMirror, opl_elf, sha256};

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

fn run(api: &Api, request: Value) -> JobInfo {
    let started = call(api, request).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    api.jobs().wait(job).unwrap()
}

fn error_key(info: &JobInfo) -> String {
    assert_eq!(info.status, JobStatus::Failed, "{info:?}");
    info.error
        .as_ref()
        .unwrap()
        .phrase
        .as_ref()
        .unwrap()
        .key
        .clone()
}

fn offer(info: JobInfo) -> LoaderOffer {
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::LoaderOffer(offer)) => *offer,
        other => panic!("unexpected {other:?}"),
    }
}

fn file(info: JobInfo) -> LoaderFile {
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::LoaderFile(file)) => *file,
        other => panic!("unexpected {other:?}"),
    }
}

struct Setup {
    api: Api,
    mirror: String,
    root: std::path::PathBuf,
    archive: Vec<u8>,
    _dir: tempfile::TempDir,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("mirror");
    let archive = ReleaseMirror::new(&root).official().unwrap();
    Setup {
        api: Api::new().with_data_dir(dir.path().join("data")),
        mirror: root.to_string_lossy().into_owned(),
        root,
        archive,
        _dir: dir,
    }
}

#[test]
fn the_catalogue_is_one_request_away() {
    let s = setup();
    let json = call(&s.api, json!({"type": "LoaderCatalog"})).unwrap();
    assert_eq!(json["type"], "LoaderCatalog");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["source"], "builtIn");
    let builds = json["builds"].as_array().unwrap();
    let ids: Vec<&str> = builds.iter().map(|b| b["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["opl-stable", "opl-beta", "yaopl"]);
    let recommended: Vec<&str> = builds
        .iter()
        .filter(|b| b["recommended"] == true)
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    assert_eq!(recommended, ["opl-beta"]);
    for build in builds {
        assert!(build["summary"]["en"].is_string() && build["summary"]["ru"].is_string());
        assert_eq!(build["license"]["spdx"], "AFL-3.0");
        let presets: Vec<&str> = build["presets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["path"].as_str().unwrap())
            .collect();
        assert!(presets.contains(&"UPDT/OPL.ELF"), "{presets:?}");
        assert!(presets.contains(&"APPS/OPNPS2LD.ELF"), "{presets:?}");
    }
    assert_eq!(builds[0]["profile"], "Opl110");
    assert_eq!(builds[0]["channel"]["tag"], "v1.1.0");
    assert_eq!(builds[0]["channel"]["floating"], false);
    assert_eq!(builds[2]["publisher"], "fork");
}

#[test]
fn a_download_is_held_to_what_the_user_saw() {
    let s = setup();
    let beta = opl_elf("v1.2.0-Beta-2271-b3a2199");
    let details = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!(details.version, "v1.2.0-Beta-2271-b3a2199");
    assert_eq!(details.date.as_deref(), Some("2026-10-13"));
    assert_eq!(details.file.name, "OPNPS2LD.ELF");
    assert_eq!(details.file.size, beta.len() as u64);
    assert_eq!(details.file.sha256.as_deref(), Some(sha256(&beta).as_str()));
    assert_eq!(details.file.sources, [LoaderChecksumSource::Digest]);
    assert_eq!(details.changelog[0].text, "Add a note by @someone");
    assert_eq!(
        details.changelog[0].link.as_deref(),
        Some("https://github.com/ps2homebrew/Open-PS2-Loader/commit/b3a2199")
    );
    assert_eq!(details.license.spdx, "AFL-3.0");
    assert!(!details.offline);
    assert_eq!(details.stored, None);

    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "sha256": details.file.sha256, "mirror": s.mirror}),
    ));
    assert_eq!(got.sha256, sha256(&beta));
    assert_eq!(got.version.as_deref(), Some("v1.2.0-Beta-2271-b3a2199"));
    assert_eq!(
        (got.build.as_deref(), got.tag.as_deref()),
        (Some("opl-beta"), Some("latest"))
    );
    assert!(got.confirmed);
    let listed = call(&s.api, json!({"type": "LoaderFiles"})).unwrap();
    assert_eq!(listed["files"][0]["sha256"], got.sha256.as_str());

    // Already stored: the details say so and a second download reuses it.
    let again = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!(again.stored.as_deref(), Some(got.sha256.as_str()));

    // The tag was rebuilt between the details and the download.
    let info = run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "sha256": "0".repeat(64), "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderReleaseChanged");
}

#[test]
fn yaopl_needs_its_sums_and_digest_to_agree() {
    let s = setup();
    let details = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "yaopl", "mirror": s.mirror}),
    ));
    assert_eq!(details.version, "v1.2.0-Beta-2370-33a2b19-yaopl");
    assert_eq!(
        details.file.sources,
        [LoaderChecksumSource::Digest, LoaderChecksumSource::Sums]
    );
    // Service lines only: no change list was published.
    assert!(details.changelog.is_empty());

    let sums = s
        .root
        .join("belomaxorka/yaopl/releases/download/latest/SHA256SUMS");
    std::fs::write(&sums, format!("{}  OPNPS2LD.ELF\n", "1".repeat(64))).unwrap();
    let info = run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "yaopl", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderChecksumsDisagree");
}

#[test]
fn a_substituted_file_is_never_stored() {
    let s = setup();
    // The mirror's 1.1.0 archive is not the real one the catalogue pins.
    let info = run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-stable", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderDownloadChecksum");
    // A beta whose file changed after GitHub's digest was published.
    std::fs::write(
        s.root
            .join("ps2homebrew/Open-PS2-Loader/releases/download/latest/OPNPS2LD.ELF"),
        opl_elf("v1.2.0-Beta-2271-b3a2199")
            .iter()
            .rev()
            .copied()
            .collect::<Vec<u8>>(),
    )
    .unwrap();
    let info = run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderDownloadChecksum");
    assert_eq!(
        call(&s.api, json!({"type": "LoaderFiles"})).unwrap()["files"],
        json!([])
    );
}

#[test]
fn requests_name_a_build_and_one_of_its_releases() {
    let s = setup();
    let info = run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "nope", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderUnknownBuild");
    let info = run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "tag": "v1.1.0", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderTagNotInChannel");
    // No such release in the mirror.
    let info = run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "yaopl", "tag": "v1.2.0-Beta-1-0123abc-yaopl", "mirror": s.mirror}),
    );
    assert_eq!(info.status, JobStatus::Failed);
}

#[test]
fn the_catalogue_from_main_is_kept_only_when_it_reads() {
    let s = setup();
    let mirror = ReleaseMirror::new(&s.root);
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/loaders");
    let stable = std::fs::read_to_string(data.join("opl-stable.toml")).unwrap();
    // `main` pins the mirror's archive: the stable build downloads from it.
    let stable = stable.replace(
        "869b51f2c9c442c79f9545a57d65ed1a28a47b19fb82ac7d307ad6cb30ddd4ed",
        &sha256(&s.archive),
    );
    let beta = std::fs::read_to_string(data.join("opl-beta.toml")).unwrap();
    mirror
        .catalog_file(
            "loaders/index.toml",
            "schema = 1\nbuilds = [\"opl-stable\", \"opl-beta\"]\n",
        )
        .unwrap();
    mirror
        .catalog_file("loaders/opl-stable.toml", &stable)
        .unwrap();
    mirror.catalog_file("loaders/opl-beta.toml", &beta).unwrap();

    let info = run(
        &s.api,
        json!({"type": "UpdateLoaderCatalog", "mirror": s.mirror}),
    );
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    let now = call(&s.api, json!({"type": "LoaderCatalog"})).unwrap();
    assert_eq!(now["source"], "main");
    assert!(now["fetchedAt"].as_u64().unwrap() > 0);
    assert_eq!(now["builds"].as_array().unwrap().len(), 2);

    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-stable", "mirror": s.mirror}),
    ));
    assert_eq!(got.version.as_deref(), Some("v1.1.0"));
    assert_eq!(got.member.as_deref(), Some("OPNPS2LD/OPNPS2LD-v1.1.0.ELF"));
    assert_eq!(got.sha256, sha256(&opl_elf("v1.1.0")));
    assert!(got.confirmed);

    // A broken catalogue on `main` leaves the stored one in force.
    mirror
        .catalog_file("loaders/opl-beta.toml", "id = \"opl-beta\"\n")
        .unwrap();
    let info = run(
        &s.api,
        json!({"type": "UpdateLoaderCatalog", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderCatalogInvalid");
    let kept = call(&s.api, json!({"type": "LoaderCatalog"})).unwrap();
    assert_eq!(kept["builds"].as_array().unwrap().len(), 2);
    // So does a catalogue for a newer program.
    mirror
        .catalog_file("loaders/index.toml", "schema = 99\nbuilds = []\n")
        .unwrap();
    let info = run(
        &s.api,
        json!({"type": "UpdateLoaderCatalog", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderCatalogInvalid");
    assert_eq!(
        serde_json::from_value::<LoaderCatalogSource>(
            call(&s.api, json!({"type": "LoaderCatalog"})).unwrap()["source"].clone()
        )
        .unwrap(),
        LoaderCatalogSource::Main
    );
}

#[test]
fn the_users_own_file_says_what_it_is() {
    let s = setup();
    let dir = tempfile::tempdir().unwrap();
    let elf = dir.path().join("OPL.ELF");
    std::fs::write(&elf, opl_elf("v1.2.0-Beta-1990-0123abc")).unwrap();
    let got = file(run(
        &s.api,
        json!({"type": "ImportLoader", "file": elf.to_string_lossy()}),
    ));
    assert_eq!(got.build.as_deref(), Some("opl-beta"));
    assert_eq!(got.version.as_deref(), Some("v1.2.0-Beta-1990-0123abc"));
    assert_eq!((got.tag, got.confirmed, got.stable), (None, false, None));
    assert_eq!(got.file, "OPL.ELF");

    let archive = dir.path().join("OPNPS2LD-v1.1.0.7z");
    std::fs::write(&archive, &s.archive).unwrap();
    let got = file(run(
        &s.api,
        json!({"type": "ImportLoader", "file": archive.to_string_lossy()}),
    ));
    assert_eq!(got.build.as_deref(), Some("opl-stable"));
    assert_eq!(got.member.as_deref(), Some("OPNPS2LD/OPNPS2LD-v1.1.0.ELF"));

    let text = dir.path().join("notes.txt");
    std::fs::write(&text, "hello").unwrap();
    let info = run(
        &s.api,
        json!({"type": "ImportLoader", "file": text.to_string_lossy()}),
    );
    assert_eq!(error_key(&info), "errors.loaderFileNoElf");
    let files = call(&s.api, json!({"type": "LoaderFiles"})).unwrap();
    assert_eq!(files["files"].as_array().unwrap().len(), 2);
}
