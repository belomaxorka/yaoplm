//! Choosing and replacing loader builds through `dispatch` (LDR-6, LDR-7):
//! the releases a channel offers, what a replacement changes for the games,
//! and the replace plan — conversions and pictures first, the loader last.
//! Releases come from a mirror folder; no test goes out.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;
use std::path::Path;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobInfo, JobStatus};
use yaoplm_api::{Api, JobId};
use yaoplm_formats::zso;
use yaoplm_test_support::Variant;
use yaoplm_test_support::loaders::{MirrorAsset, ReleaseMirror, opl_elf};

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

fn run(api: &Api, request: Value) -> JobInfo {
    let started = call(api, request).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    info
}

fn result(info: JobInfo) -> Value {
    serde_json::to_value(info.result.unwrap()).unwrap()
}

fn execute(api: &Api, plan: &Value, confirm_delete: bool) {
    run(
        api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": confirm_delete}),
    );
}

fn titles(plan: &Value) -> Vec<String> {
    plan["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].as_str().unwrap().to_owned())
        .collect()
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn zso_of(iso: &[u8]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    zso::compress(
        &mut Cursor::new(iso),
        iso.len() as u64,
        &mut out,
        &mut |_| {},
    )
    .unwrap();
    out.into_inner()
}

fn jpeg() -> Vec<u8> {
    let image = image::RgbImage::from_pixel(140, 200, image::Rgb([200, 40, 40]));
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut out, image::ImageFormat::Jpeg)
        .unwrap();
    out.into_inner()
}

struct Setup {
    api: Api,
    mirror: String,
    dir: tempfile::TempDir,
    device: std::path::PathBuf,
    root: String,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let mirror = dir.path().join("mirror");
    ReleaseMirror::new(&mirror).official().unwrap();
    let device = dir.path().join("stick");
    std::fs::create_dir_all(&device).unwrap();
    Setup {
        api: Api::new().with_data_dir(dir.path().join("data")),
        mirror: mirror.to_string_lossy().into_owned(),
        root: device.to_string_lossy().into_owned(),
        device,
        dir,
    }
}

impl Setup {
    /// The beta from the mirror, stored.
    fn beta(&self) -> String {
        let got = result(run(
            &self.api,
            json!({"type": "DownloadLoader", "build": "opl-beta", "mirror": self.mirror}),
        ));
        got["sha256"].as_str().unwrap().to_owned()
    }

    /// OPL 1.1.0 as the user's own file: the catalogue pins the real
    /// release's archive, which the mirror does not have.
    fn stable(&self) -> String {
        let file = self.dir.path().join("OPNPS2LD-v1.1.0.ELF");
        std::fs::write(&file, opl_elf("v1.1.0")).unwrap();
        let got = result(run(
            &self.api,
            json!({"type": "ImportLoader", "file": file.to_string_lossy()}),
        ));
        assert_eq!(got["build"], "opl-stable");
        got["sha256"].as_str().unwrap().to_owned()
    }

    fn install(&self, build: &str, sha256: &str) {
        let plan = call(
            &self.api,
            json!({"type": "PlanInstallLoader", "root": self.root, "build": build, "sha256": sha256,
                   "presets": ["fmcb"], "device": "usb"}),
        )
        .unwrap();
        execute(&self.api, &plan, false);
    }

    fn impact(&self, build: &str, profile: &str) -> Value {
        result(run(
            &self.api,
            json!({"type": "LoaderImpact", "root": self.root, "build": build, "profile": profile}),
        ))
    }
}

#[test]
fn moving_to_opl_110_converts_zso_first_and_puts_the_loader_last() {
    let s = setup();
    let iso = Variant::Retail.build().to_vec();
    write(
        &s.device.join("DVD/SLUS_200.62.Retail Game.zso"),
        &zso_of(&iso),
    );
    let beta = s.beta();
    s.install("opl-beta", &beta);

    let impact = s.impact("opl-stable", "Opl120Beta");
    assert_eq!(impact["type"], "LoaderImpact");
    assert_eq!(
        (impact["from"].clone(), impact["to"].clone()),
        (json!("Opl120Beta"), json!("Opl110"))
    );
    assert_eq!(impact["blocked"], Value::Null);
    assert_eq!(impact["zso"][0]["path"], "DVD/SLUS_200.62.Retail Game.zso");
    assert_eq!(impact["art"], json!([]));

    let stable = s.stable();
    let plan = call(
        &s.api,
        json!({"type": "PlanReplaceLoader", "root": s.root, "build": "opl-stable", "sha256": stable,
               "presets": ["fmcb"], "device": "usb", "profile": "Opl120Beta",
               "convert": ["DVD/SLUS_200.62.Retail Game.zso"], "reencodeArt": true}),
    )
    .unwrap();
    assert_eq!(plan["kind"], "replaceLoader");
    assert_eq!(plan["title"], "Replace OPL 1.2.0 beta with OPL 1.1.0");
    assert_eq!(plan["executable"], true, "{plan}");
    // The ZSO goes once its ISO is complete.
    assert_eq!(plan["deletesUserData"], true);
    let titles = titles(&plan);
    assert!(titles[0].contains("SLUS_200.62"), "{titles:?}");
    assert_eq!(titles.last().unwrap(), "Remember the installed loader");
    assert_eq!(titles[titles.len() - 2], "Put OPL 1.1.0 in UPDT/OPL.ELF");
    execute(&s.api, &plan, true);

    let at = |p: &str| std::fs::read(s.device.join(p)).unwrap();
    assert_eq!(at("DVD/SLUS_200.62.Retail Game.iso"), iso);
    assert!(!s.device.join("DVD/SLUS_200.62.Retail Game.zso").exists());
    assert_eq!(at("UPDT/OPL.ELF"), opl_elf("v1.1.0"));
    let card = call(&s.api, json!({"type": "InspectDevice", "root": s.root})).unwrap();
    let loader = &card["installed"][0];
    assert_eq!(loader["build"], "opl-stable");
    assert_eq!(loader["backup"]["version"], "v1.2.0-Beta-2271-b3a2199");
    assert_eq!(card["loader"]["profile"], "Opl110");
}

fn codes(plan: &Value) -> Vec<String> {
    plan["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["code"].as_str().unwrap().to_owned())
        .collect()
}

fn notice<'a>(plan: &'a Value, code: &str) -> &'a Value {
    plan["notices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["code"] == code)
        .unwrap_or_else(|| panic!("no {code} in {plan}"))
}

#[test]
fn rolling_back_to_another_build_says_what_changes_for_the_games() {
    // A rollback across builds checks what replacing does (LDR-4, LDR-7):
    // on main it planned the old ELF back without a word about the games.
    let s = setup();
    let iso = Variant::Retail.build().to_vec();
    write(
        &s.device.join("DVD/SLUS_200.62.Retail Game.zso"),
        &zso_of(&iso),
    );
    let stable = s.stable();
    s.install("opl-stable", &stable);
    let beta = s.beta();
    s.install("opl-beta", &beta);
    let rollback = || {
        call(
            &s.api,
            json!({"type": "PlanRollbackLoader", "root": s.root, "path": "UPDT/OPL.ELF"}),
        )
        .unwrap()
    };

    // Back to 1.1.0: the ZSO game would vanish; "Replace…" converts it.
    let plan = rollback();
    let zso = notice(&plan, "rollbackZso");
    assert_eq!(zso["level"], "warning");
    assert_eq!(
        zso["message"],
        "ZSO games OPL 1.1.0 cannot show: 1. “Replace…” converts them to ISO in the same plan"
    );
    // A warning: the user may still go back.
    assert_eq!(plan["executable"], true, "{plan}");
    execute(&s.api, &plan, false);

    // Back to the beta: a JPG cover would not load; "Replace…" re-encodes.
    write(&s.device.join("ART/SLUS_200.62_COV.jpg"), &jpeg());
    let plan = rollback();
    assert_eq!(notice(&plan, "rollbackArt")["level"], "warning");
    assert!(
        !codes(&plan).contains(&"rollbackZso".to_owned()),
        "the beta shows ZSO: {plan}"
    );
}

#[test]
fn leaving_opl_110_re_encodes_its_jpg_pictures() {
    let s = setup();
    write(
        &s.device.join("DVD/SLUS_200.62.Retail Game.iso"),
        &Variant::Retail.build().to_vec(),
    );
    write(&s.device.join("ART/SLUS_200.62_COV.jpg"), &jpeg());
    let stable = s.stable();
    s.install("opl-stable", &stable);

    let impact = s.impact("opl-beta", "Opl110");
    assert_eq!(impact["zso"], json!([]));
    assert_eq!(impact["art"][0]["path"], "ART/SLUS_200.62_COV.jpg");
    assert_eq!(impact["art"][0]["suffix"], "COV");

    let beta = s.beta();
    let plan = call(
        &s.api,
        json!({"type": "PlanReplaceLoader", "root": s.root, "build": "opl-beta", "sha256": beta,
               "presets": ["fmcb"], "device": "usb", "profile": "Opl110", "reencodeArt": true}),
    )
    .unwrap();
    assert_eq!(plan["title"], "Replace OPL 1.1.0 with OPL 1.2.0 beta");
    assert_eq!(plan["deletesUserData"], false, "the JPG goes to a backup");
    execute(&s.api, &plan, false);
    let png = std::fs::read(s.device.join("ART/SLUS_200.62_COV.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG"));
    assert!(!s.device.join("ART/SLUS_200.62_COV.jpg").exists());
    assert_eq!(
        std::fs::read(s.device.join("UPDT/OPL.ELF")).unwrap(),
        opl_elf("v1.2.0-Beta-2271-b3a2199")
    );

    // Left out: the picture stays as it is.
    let s = setup();
    write(&s.device.join("ART/SLUS_200.62_COV.jpg"), &jpeg());
    let stable = s.stable();
    s.install("opl-stable", &stable);
    let beta = s.beta();
    let plan = call(
        &s.api,
        json!({"type": "PlanReplaceLoader", "root": s.root, "build": "opl-beta", "sha256": beta,
               "presets": ["fmcb"], "device": "usb", "profile": "Opl110", "reencodeArt": false}),
    )
    .unwrap();
    execute(&s.api, &plan, false);
    assert!(s.device.join("ART/SLUS_200.62_COV.jpg").exists());
}

#[test]
fn the_same_rules_change_nothing_and_a_path_left_behind_can_be_retired() {
    let s = setup();
    let beta = s.beta();
    s.install("opl-beta", &beta);
    let impact = s.impact("yaopl", "Opl120Beta");
    for list in ["zso", "fragmented", "unknownFragments", "art"] {
        assert_eq!(impact[list], json!([]), "{list}");
    }
    let yaopl = result(run(
        &s.api,
        json!({"type": "DownloadLoader", "build": "yaopl", "mirror": s.mirror}),
    ));
    // YAOPL as an app, the beta at FMCB's path moved into the backup.
    let plan = call(
        &s.api,
        json!({"type": "PlanReplaceLoader", "root": s.root, "build": "yaopl", "sha256": yaopl["sha256"],
               "presets": ["app"], "device": "usb", "profile": "Opl120Beta", "retire": ["UPDT/OPL.ELF"]}),
    )
    .unwrap();
    assert_eq!(plan["title"], "Replace OPL 1.2.0 beta with YAOPL");
    assert!(titles(&plan).contains(&"Move UPDT/OPL.ELF into the backup folder".to_owned()));
    execute(&s.api, &plan, false);
    assert!(s.device.join("APPS/YAOPL/OPNPS2LD.ELF").is_file());
    assert!(!s.device.join("UPDT/OPL.ELF").exists());
    let card = call(&s.api, json!({"type": "InspectDevice", "root": s.root})).unwrap();
    let installed: Vec<&str> = card["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["path"].as_str().unwrap())
        .collect();
    assert_eq!(installed, ["APPS/YAOPL/OPNPS2LD.ELF"]);
}

#[test]
fn a_channel_offers_only_the_releases_the_catalogue_allows() {
    let s = setup();
    let mirror = ReleaseMirror::new(Path::new(&s.mirror));
    let older = opl_elf("v2.0.0-rc1-yaopl");
    mirror
        .release(
            "yaopl/yaopl",
            "yaopl-v2.0.0-rc1",
            "v2.0.0-rc1-yaopl",
            "",
            "2026-10-20",
            &[MirrorAsset {
                name: "OPNPS2LD.ELF",
                bytes: &older,
                digest: true,
            }],
        )
        .unwrap();
    // Upstream keeps 1.0.0 too, which no profile of YAOPLM follows.
    mirror
        .release(
            "ps2homebrew/Open-PS2-Loader",
            "v1.0.0",
            "v1.0.0",
            "",
            "2021-01-06",
            &[],
        )
        .unwrap();
    let list = |build: &str| {
        let got = result(run(
            &s.api,
            json!({"type": "LoaderReleases", "build": build, "mirror": s.mirror}),
        ));
        got["releases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                (
                    r["tag"].as_str().unwrap().to_owned(),
                    r["version"].as_str().map(str::to_owned),
                    r["default"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        list("yaopl"),
        [
            (
                "latest".to_owned(),
                Some("v1.2.0-Beta-2370-33a2b19-yaopl".to_owned()),
                true
            ),
            (
                "yaopl-v2.0.0-rc1".to_owned(),
                Some("v2.0.0-rc1-yaopl".to_owned()),
                false
            ),
        ]
    );
    let beta = list("opl-beta");
    assert_eq!(beta.len(), 1);
    assert_eq!((beta[0].0.as_str(), beta[0].2), ("latest", true));
    let stable = list("opl-stable");
    assert_eq!(stable.len(), 1);
    assert_eq!(stable[0].0, "v1.1.0");
    // The YAOPL candidate is one offer away, held to its own hash.
    let offer = result(run(
        &s.api,
        json!({"type": "LoaderRelease", "build": "yaopl", "tag": "yaopl-v2.0.0-rc1", "mirror": s.mirror}),
    ));
    assert_eq!(offer["version"], "v2.0.0-rc1-yaopl");
}

#[test]
fn rolling_back_to_another_build_names_the_builds_and_says_when_nothing_changes() {
    // The plan says which build comes back in place of which, as the
    // replace window does, and that no game is affected when none is
    // (release pass 1.4). On main the title named only the path and the
    // plan was silent about the games.
    let s = setup();
    let stable = s.stable();
    s.install("opl-stable", &stable);
    let beta = s.beta();
    s.install("opl-beta", &beta);
    let plan = call(
        &s.api,
        json!({"type": "PlanRollbackLoader", "root": s.root, "path": "UPDT/OPL.ELF"}),
    )
    .unwrap();
    assert_eq!(plan["kind"], "rollbackLoader");
    assert_eq!(
        plan["title"],
        "Put back OPL 1.1.0 in place of OPL 1.2.0 beta"
    );
    let nothing = notice(&plan, "rollbackNothingAffected");
    assert_eq!(nothing["level"], "info");
    assert_eq!(
        nothing["message"],
        "The rules of OPL 1.1.0 differ, but none of the games on this device is affected"
    );
    assert_eq!(plan["executable"], true, "{plan}");

    // With a ZSO game the warning stands alone.
    write(
        &s.device.join("DVD/SLUS_200.62.Retail Game.zso"),
        &zso_of(&Variant::Retail.build().to_vec()),
    );
    let plan = call(
        &s.api,
        json!({"type": "PlanRollbackLoader", "root": s.root, "path": "UPDT/OPL.ELF"}),
    )
    .unwrap();
    assert!(codes(&plan).contains(&"rollbackZso".to_owned()), "{plan}");
    assert!(
        !codes(&plan).contains(&"rollbackNothingAffected".to_owned()),
        "{plan}"
    );
}
