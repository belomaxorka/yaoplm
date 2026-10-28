//! Loaders through `dispatch` (LDR-1 to LDR-4): the catalogue, what a
//! download would be, downloads held to the hash the user saw, the user's
//! own files, the catalogue from `main`, installs, updates and rollbacks. Releases come from a mirror folder with
//! GitHub's layout: no test goes out (CI sets `YAOPLM_OFFLINE`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobInfo, JobResult, JobStatus};
use yaoplm_api::loaders::{LoaderCatalogSource, LoaderChecksumSource, LoaderFile, LoaderOffer};
use yaoplm_api::{Api, JobId};
use yaoplm_test_support::loaders::{MirrorAsset, ReleaseMirror, opl_elf, sha256};

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

#[test]
fn a_versioned_yaopl_release_is_one_of_its_channel() {
    let s = setup();
    let elf = opl_elf("v2.0.0-yaopl");
    let sums = format!("{}  OPNPS2LD.ELF\n", sha256(&elf));
    ReleaseMirror::new(&s.root)
        .release(
            "belomaxorka/yaopl",
            "yaopl-v2.0.0",
            "v2.0.0-yaopl",
            "- Fix a hang on an empty cheat name\n",
            "2026-11-01",
            &[
                MirrorAsset {
                    name: "OPNPS2LD.ELF",
                    bytes: &elf,
                    digest: true,
                },
                MirrorAsset {
                    name: "SHA256SUMS",
                    bytes: sums.as_bytes(),
                    digest: true,
                },
            ],
        )
        .unwrap();
    // A YAOPL release: tag `yaopl-vX.Y.Z`, named by its version (ADR-0051).
    let (tag, version) = ("yaopl-v2.0.0", "v2.0.0-yaopl");
    let details = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "yaopl", "tag": tag, "mirror": s.mirror}),
    ));
    assert_eq!(
        (details.tag.as_str(), details.version.as_str()),
        (tag, version)
    );
    assert_eq!(
        details.changelog[0].text,
        "Fix a hang on an empty cheat name"
    );
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "yaopl", "tag": tag, "sha256": details.file.sha256, "mirror": s.mirror}),
    ));
    assert_eq!(got.tag.as_deref(), Some(tag));
    assert_eq!(got.version.as_deref(), Some(version));
}

#[test]
fn a_release_without_checksums_is_kept_but_not_confirmed() {
    let s = setup();
    let elf = opl_elf("v1.2.0-Beta-2280-0123abc");
    ReleaseMirror::new(&s.root)
        .release(
            "ps2homebrew/Open-PS2-Loader",
            "latest",
            "latest",
            "",
            "2026-11-02",
            &[
                MirrorAsset {
                    name: "OPNPS2LD.ELF",
                    bytes: &elf,
                    digest: false,
                },
                MirrorAsset {
                    name: "OPNPS2LD-v1.2.0-Beta-2280-0123abc.7z",
                    bytes: b"7z",
                    digest: false,
                },
            ],
        )
        .unwrap();
    let details = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!(details.file.sha256, None);
    assert!(details.file.sources.is_empty() && details.changelog.is_empty());
    // A hash the user could not have seen is still held to.
    let info = run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "sha256": "0".repeat(64), "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderDownloadChecksum");
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!(got.sha256, sha256(&elf));
    assert!(!got.confirmed);
}

#[test]
fn broken_and_oversized_releases_are_refused_before_a_download() {
    let s = setup();
    let tags = s
        .root
        .join("repos/ps2homebrew/Open-PS2-Loader/releases/tags");
    std::fs::write(tags.join("latest.json"), "{\"tag_name\": \"latest\"").unwrap();
    let info = run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderReleaseUnreadable");

    // The release claims a 100 MiB ELF: it is never asked for.
    let big = format!(
        "{{\"tag_name\":\"latest\",\"name\":\"latest\",\"body\":\"\",\"html_url\":\"https://github.com/ps2homebrew/Open-PS2-Loader/releases/tag/latest\",\"assets\":[{{\"name\":\"OPNPS2LD.ELF\",\"size\":104857600,\"digest\":\"sha256:{0}\",\"browser_download_url\":\"https://github.com/ps2homebrew/Open-PS2-Loader/releases/download/latest/OPNPS2LD.ELF\"}},{{\"name\":\"OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z\",\"size\":1,\"browser_download_url\":\"https://github.com/ps2homebrew/Open-PS2-Loader/releases/download/latest/OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z\"}}]}}",
        "a".repeat(64)
    );
    std::fs::write(tags.join("latest.json"), big).unwrap();
    let info = run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderFileTooLarge");
}

#[test]
fn an_oversized_own_file_is_not_read() {
    let s = setup();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("OPNPS2LD.ELF");
    // Sparse: no 65 MiB are written.
    std::fs::File::create(&path)
        .unwrap()
        .set_len(65 << 20)
        .unwrap();
    let info = run(
        &s.api,
        json!({"type": "ImportLoader", "file": path.to_string_lossy()}),
    );
    assert_eq!(error_key(&info), "errors.loaderFileTooLarge");
}

#[test]
fn a_catalogue_that_names_a_missing_file_is_refused() {
    let s = setup();
    let mirror = ReleaseMirror::new(&s.root);
    mirror
        .catalog_file(
            "loaders/index.toml",
            "schema = 1\nbuilds = [\"opl-beta\", \"gone\"]\n",
        )
        .unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/loaders/opl-beta.toml");
    mirror
        .catalog_file(
            "loaders/opl-beta.toml",
            &std::fs::read_to_string(data).unwrap(),
        )
        .unwrap();
    let info = run(
        &s.api,
        json!({"type": "UpdateLoaderCatalog", "mirror": s.mirror}),
    );
    assert_eq!(error_key(&info), "errors.loaderCatalogInvalid");
    let message = &info.error.as_ref().unwrap().message;
    assert!(message.contains("loaders/gone.toml"), "{message}");
    assert_eq!(
        call(&s.api, json!({"type": "LoaderCatalog"})).unwrap()["source"],
        "builtIn"
    );
}

#[test]
fn importing_a_downloaded_build_keeps_what_its_release_said() {
    let s = setup();
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "yaopl", "mirror": s.mirror}),
    ));
    assert!(got.confirmed);
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("OPNPS2LD.ELF");
    std::fs::write(&copy, opl_elf("v1.2.0-Beta-2370-33a2b19-yaopl")).unwrap();
    let again = file(run(
        &s.api,
        json!({"type": "ImportLoader", "file": copy.to_string_lossy()}),
    ));
    assert_eq!(
        (again.tag.as_deref(), again.confirmed),
        (Some("latest"), true)
    );
    let files = call(&s.api, json!({"type": "LoaderFiles"})).unwrap();
    assert_eq!(files["files"].as_array().unwrap().len(), 1);
    assert_eq!(files["files"][0]["tag"], "latest");

    // The other way round: a download confirms what an import left.
    let s = setup();
    let own = dir.path().join("beta.ELF");
    std::fs::write(&own, opl_elf("v1.2.0-Beta-2271-b3a2199")).unwrap();
    let first = file(run(
        &s.api,
        json!({"type": "ImportLoader", "file": own.to_string_lossy()}),
    ));
    assert!(!first.confirmed);
    let then = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!((then.sha256, then.confirmed), (first.sha256, true));
}

fn execute(api: &Api, plan: &Value) -> JobInfo {
    run(
        api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": false}),
    )
}

fn codes(plan: &Value) -> Vec<String> {
    plan["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_loader_is_installed_through_a_plan_with_its_boot_hints() {
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    let root = device.path().to_string_lossy().into_owned();
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "languages": true, "mirror": s.mirror}),
    ));
    assert!(got.languages);
    let languages = call(
        &s.api,
        json!({"type": "LoaderLanguages", "sha256": got.sha256}),
    )
    .unwrap();
    assert_eq!(languages["languages"], json!(["German", "Russian"]));

    let plan = call(
        &s.api,
        json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": got.sha256,
               "presets": ["fmcb", "ps2bbl"], "device": "usb", "language": "Russian", "starterSettings": true}),
    )
    .unwrap();
    assert_eq!(plan["type"], "Plan");
    assert_eq!(plan["kind"], "installLoader");
    assert_eq!(plan["executable"], true, "{plan}");
    let info = execute(&s.api, &plan);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");

    let beta = opl_elf("v1.2.0-Beta-2271-b3a2199");
    let at = |p: &str| std::fs::read(device.path().join(p)).unwrap();
    assert_eq!(at("UPDT/OPL.ELF"), beta);
    assert_eq!(at("APPS/OPNPS2LD.ELF"), beta);
    assert_eq!(at("LNG/lang_Russian.lng"), b"Russian strings");
    assert_eq!(at("LNG/font_Russian.ttf"), b"Russian font");
    let cfg = String::from_utf8(at("conf_opl.cfg")).unwrap();
    assert!(
        cfg.contains("usb_mode=2") && cfg.contains("language_text=Russian"),
        "{cfg}"
    );
    let manifest: Value = serde_json::from_slice(&at(".yaoplm/manifest.json")).unwrap();
    assert_eq!(manifest["loaders"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["loaders"][0]["language"], "Russian");
    for folder in ["CD", "DVD", "ART", "CFG", "VMC"] {
        assert!(device.path().join(folder).is_dir(), "{folder}");
    }

    let hints = call(
        &s.api,
        json!({"type": "LoaderBootHints", "build": "opl-beta", "presets": ["fmcb", "ps2bbl"], "device": "usb"}),
    )
    .unwrap();
    let hints = hints["hints"].as_array().unwrap();
    assert_eq!(hints.len(), 2);
    assert!(hints.iter().all(|h| h["stock"] == true));
    assert_eq!(hints[0]["ps2Path"], "mass:/UPDT/OPL.ELF");
    let mx = call(
        &s.api,
        json!({"type": "LoaderBootHints", "build": "opl-beta", "presets": ["app"], "device": "mx4sio"}),
    )
    .unwrap();
    let lines: Vec<&str> = mx["hints"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|h| {
            h["lines"]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap())
        })
        .collect();
    assert!(
        lines.contains(&"LK_AUTO_E1 = massX:/APPS/OPL/OPNPS2LD.ELF"),
        "{lines:?}"
    );
}

#[test]
fn a_foreign_file_blocks_the_plan_until_confirmed() {
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    let root = device.path().to_string_lossy().into_owned();
    std::fs::create_dir_all(device.path().join("APPS")).unwrap();
    let foreign = yaoplm_test_support::loaders::plain_elf(&["LAUNCHELF.CNF", "wLaunchELF"]);
    std::fs::write(device.path().join("APPS/OPNPS2LD.ELF"), &foreign).unwrap();
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    let request = json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": got.sha256,
                         "presets": ["ps2bbl"], "device": "usb"});
    let plan = call(&s.api, request.clone()).unwrap();
    assert_eq!(plan["executable"], false);
    assert!(
        codes(&plan).contains(&"loaderForeignFile".to_owned()),
        "{plan}"
    );
    // A plan that cannot run is refused outright.
    let refused = call(
        &s.api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": false}),
    );
    assert!(refused.is_err());
    assert_eq!(
        std::fs::read(device.path().join("APPS/OPNPS2LD.ELF")).unwrap(),
        foreign
    );

    let mut confirmed = request;
    confirmed["replaceForeign"] = json!(true);
    let plan = call(&s.api, confirmed).unwrap();
    assert_eq!(plan["executable"], true, "{plan}");
    assert_eq!(execute(&s.api, &plan).status, JobStatus::Finished);
    let backups: Vec<_> = walk(&device.path().join(".yaoplm/backup/loader"));
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(std::fs::read(&backups[0]).unwrap(), foreign);
}

#[test]
fn a_memory_card_emulator_s_sd_gets_no_loader() {
    // ADR-0048: on main this plan put OPL into `UPDT/` next to
    // `MemoryCards/`, where nothing starts it.
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    yaoplm_test_support::emulator::write_to(device.path()).unwrap();
    let root = device.path().to_string_lossy().into_owned();
    let got = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    for presets in [json!(["fmcb"]), json!(["ps2bbl", "app"])] {
        let plan = call(
            &s.api,
            json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": got.sha256,
                   "presets": presets, "device": "usb"}),
        )
        .unwrap();
        assert_eq!(codes(&plan), ["loaderOnEmulatorSd"], "{plan}");
        assert_eq!(plan["executable"], false);
    }
    assert!(!device.path().join("UPDT").exists());
    assert!(!device.path().join("APPS").exists());
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn install_requests_name_what_is_stored_and_where_it_may_go() {
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    let root = device.path().to_string_lossy().into_owned();
    let beta = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    let yaopl = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "yaopl", "mirror": s.mirror}),
    ));
    let plan = |build: &str, sha: &str, extra: Value| {
        let mut request = json!({"type": "PlanInstallLoader", "root": root, "build": build, "sha256": sha,
                                 "presets": ["fmcb"], "device": "usb"});
        for (k, v) in extra.as_object().unwrap() {
            request[k] = v.clone();
        }
        call(&s.api, request)
            .err()
            .and_then(|e| e.phrase)
            .map(|p| p.key)
            .unwrap_or_default()
    };
    assert_eq!(
        plan("opl-beta", &"0".repeat(64), json!({})),
        "errors.loaderNotStored"
    );
    assert_eq!(
        plan("opl-beta", &yaopl.sha256, json!({})),
        "errors.loaderFileOtherBuild"
    );
    assert_eq!(
        plan("opl-beta", &beta.sha256, json!({"presets": ["nope"]})),
        "errors.loaderUnknownPreset"
    );
    assert_eq!(
        plan("opl-beta", &beta.sha256, json!({"device": "mx4sio"})),
        "errors.loaderPresetNotForDevice"
    );
    assert_eq!(
        plan("opl-beta", &beta.sha256, json!({"language": "Russian"})),
        "errors.loaderLanguagesNotStored"
    );
    // A path of the user's own is fine; one into an emulator's files is not.
    let ok = call(
        &s.api,
        json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": beta.sha256,
               "path": "/GAMES\\OPL.ELF", "device": "usb"}),
    )
    .unwrap();
    assert_eq!(ok["executable"], true, "{ok}");
    let no = call(
        &s.api,
        json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": beta.sha256,
               "path": "MemoryCards/OPL.ELF", "device": "usb"}),
    )
    .unwrap();
    assert!(
        codes(&no).contains(&"loaderPathReserved".to_owned()),
        "{no}"
    );
}

fn inspect(api: &Api, root: &str) -> Value {
    call(api, json!({"type": "InspectDevice", "root": root})).unwrap()
}

#[test]
fn the_card_learns_of_an_update_from_the_cache_and_rolls_it_back() {
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    let root = device.path().to_string_lossy().into_owned();
    let old = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    let plan = call(
        &s.api,
        json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": old.sha256,
               "presets": ["fmcb"], "device": "usb"}),
    )
    .unwrap();
    assert_eq!(execute(&s.api, &plan).status, JobStatus::Finished);

    let card = inspect(&s.api, &root);
    let loader = &card["installed"][0];
    assert_eq!(loader["path"], "UPDT/OPL.ELF");
    assert_eq!(loader["build"], "opl-beta");
    assert_eq!(loader["knownBy"], "manifest");
    assert_eq!(loader["version"], "v1.2.0-Beta-2271-b3a2199");
    assert_eq!(loader["preset"], "fmcb");
    assert_eq!(loader["device"], "usb");
    // The download read the release: the card knows it is the newest.
    assert_eq!(loader["freshness"], "current");
    assert_eq!(loader["latest"], "v1.2.0-Beta-2271-b3a2199");
    assert!(loader["checkedAt"].as_u64().is_some());
    assert_eq!(loader["backup"], Value::Null);

    // `latest` is rebuilt; the card does not ask GitHub on its own.
    ReleaseMirror::new(&s.root)
        .beta("v1.2.0-Beta-2280-0123abc", "2026-10-20")
        .unwrap();
    assert_eq!(
        inspect(&s.api, &root)["installed"][0]["freshness"],
        "current"
    );
    // "Check for updates" reads the release; from then on the card knows.
    let checked = offer(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "opl-beta", "mirror": s.mirror}),
    ));
    assert_eq!(checked.version, "v1.2.0-Beta-2280-0123abc");
    let loader = inspect(&s.api, &root)["installed"][0].clone();
    assert_eq!(loader["freshness"], "update");
    assert_eq!(loader["latest"], "v1.2.0-Beta-2280-0123abc");
    let cached = call(
        &s.api,
        json!({"type": "CachedLoaderRelease", "build": "opl-beta"}),
    )
    .unwrap();
    assert_eq!(cached["offer"]["version"], "v1.2.0-Beta-2280-0123abc");
    assert_eq!(cached["offer"]["offline"], false);
    assert_eq!(
        cached["offer"]["changelog"][0]["link"],
        "https://github.com/ps2homebrew/Open-PS2-Loader/commit/0123abc"
    );

    // The update: the newer ELF, held to the hash the window showed.
    let new = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "sha256": cached["offer"]["file"]["sha256"],
               "mirror": s.mirror}),
    ));
    let plan = call(
        &s.api,
        json!({"type": "PlanUpdateLoader", "root": root, "path": "UPDT/OPL.ELF", "sha256": new.sha256}),
    )
    .unwrap();
    assert_eq!(plan["kind"], "updateLoader");
    assert_eq!(
        plan["title"],
        "Update OPL 1.2.0 beta to v1.2.0-Beta-2280-0123abc"
    );
    assert_eq!(execute(&s.api, &plan).status, JobStatus::Finished);
    let at = |p: &str| std::fs::read(device.path().join(p)).unwrap();
    assert_eq!(at("UPDT/OPL.ELF"), opl_elf("v1.2.0-Beta-2280-0123abc"));
    let loader = inspect(&s.api, &root)["installed"][0].clone();
    assert_eq!(loader["version"], "v1.2.0-Beta-2280-0123abc");
    assert_eq!(loader["freshness"], "current");
    assert_eq!(loader["backup"]["version"], "v1.2.0-Beta-2271-b3a2199");
    assert_eq!(loader["backup"]["family"], "OPL");

    // Roll back: the older build is back, and the card offers the update again.
    let plan = call(
        &s.api,
        json!({"type": "PlanRollbackLoader", "root": root, "path": "UPDT/OPL.ELF"}),
    )
    .unwrap();
    assert_eq!(plan["kind"], "rollbackLoader");
    assert_eq!(execute(&s.api, &plan).status, JobStatus::Finished);
    assert_eq!(at("UPDT/OPL.ELF"), opl_elf("v1.2.0-Beta-2271-b3a2199"));
    let loader = inspect(&s.api, &root)["installed"][0].clone();
    assert_eq!(loader["version"], "v1.2.0-Beta-2271-b3a2199");
    assert_eq!(loader["knownBy"], "manifest");
    assert_eq!(loader["freshness"], "update");
    assert_eq!(loader["backup"]["version"], "v1.2.0-Beta-2280-0123abc");
}

#[test]
fn an_update_stays_in_the_channel_of_what_is_installed() {
    let s = setup();
    let device = tempfile::tempdir().unwrap();
    let root = device.path().to_string_lossy().into_owned();
    let beta = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": s.mirror}),
    ));
    let yaopl = file(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "yaopl", "mirror": s.mirror}),
    ));
    let key = |err: yaoplm_api::ApiError| err.phrase.unwrap().key;

    // Nothing at the path yet.
    let err = call(
        &s.api,
        json!({"type": "PlanUpdateLoader", "root": root, "path": "UPDT/OPL.ELF", "sha256": beta.sha256}),
    )
    .unwrap_err();
    assert_eq!(key(err), "errors.loaderNotInstalled");

    let plan = call(
        &s.api,
        json!({"type": "PlanInstallLoader", "root": root, "build": "opl-beta", "sha256": beta.sha256,
               "presets": ["fmcb"], "device": "usb"}),
    )
    .unwrap();
    assert_eq!(execute(&s.api, &plan).status, JobStatus::Finished);
    // YAOPL is another channel: "Replace…", never "Update".
    let err = call(
        &s.api,
        json!({"type": "PlanUpdateLoader", "root": root, "path": "UPDT/OPL.ELF", "sha256": yaopl.sha256}),
    )
    .unwrap_err();
    assert_eq!(key(err), "errors.loaderFileOtherBuild");
    // No backup yet: the rollback plan says so and cannot run.
    let plan = call(
        &s.api,
        json!({"type": "PlanRollbackLoader", "root": root, "path": "UPDT/OPL.ELF"}),
    )
    .unwrap();
    assert_eq!(plan["executable"], false);
    assert_eq!(codes(&plan), ["loaderNoBackup"]);
    // A device without loaders, and a build with no release read yet.
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(
        inspect(&s.api, &empty.path().to_string_lossy())["installed"],
        json!([])
    );
    let cached = call(
        &s.api,
        json!({"type": "CachedLoaderRelease", "build": "opl-stable"}),
    )
    .unwrap();
    assert_eq!(cached["offer"], Value::Null);
}
