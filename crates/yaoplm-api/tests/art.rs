//! Covers through `dispatch`: FetchArt as a job ending in a plan, SetArt
//! from a file and from bytes, DeleteArt, the attribution screen and the
//! cache (ART-1 to ART-4, META-1). Only local mirrors, never the network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use base64::Engine;
use image::{DynamicImage, ImageBuffer, Rgb};
use serde_json::{Value, json};
use yaoplm_api::jobs::{JobLogEntry, JobResult, JobStatus};
use yaoplm_api::{Api, ErrorCode, JobId};
use yaoplm_formats::art::png_header;
use yaoplm_test_support::Variant;

fn image_bytes(width: u32, height: u32, format: image::ImageFormat) -> Vec<u8> {
    let img = ImageBuffer::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, 42]));
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), format)
        .unwrap();
    out
}

struct Setup {
    _dir: tempfile::TempDir,
    api: Api,
    device: String,
    mirror: PathBuf,
}

/// A device with three games: the Retail one already has a cover; the
/// xlenore mirror has Retail (SLUS-20062), the libretro mirror has Dead or
/// Alive 2 (SCES-50003) by its Redump name; the DVD-9 one is nowhere.
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    for folder in ["CD", "DVD", "ART"] {
        fs::create_dir_all(device.join(folder)).unwrap();
    }
    fs::write(
        device.join("CD/SLUS_200.62.Retail.iso"),
        Variant::Retail.build().to_vec(),
    )
    .unwrap();
    fs::write(
        device.join("CD/SCES_500.03.Lf Game.iso"),
        Variant::LfOnly.build().to_vec(),
    )
    .unwrap();
    fs::write(
        device.join("DVD/SLUS_212.18.Dvd9.iso"),
        Variant::Dvd9.build().to_vec(),
    )
    .unwrap();
    fs::write(
        device.join("ART/SLUS_200.62_COV.png"),
        image_bytes(140, 200, image::ImageFormat::Png),
    )
    .unwrap();

    let mirror = dir.path().join("mirror");
    let xlenore = mirror.join("xlenore/covers/default");
    fs::create_dir_all(&xlenore).unwrap();
    fs::write(
        xlenore.join("SLUS-20062.jpg"),
        image_bytes(300, 420, image::ImageFormat::Jpeg),
    )
    .unwrap();
    let libretro = mirror.join("libretro/Named_Boxarts");
    fs::create_dir_all(&libretro).unwrap();
    fs::write(
        libretro.join("Dead or Alive 2 (Europe) (En,Fr,De,Es,It).png"),
        image_bytes(256, 362, image::ImageFormat::Png),
    )
    .unwrap();

    let api = Api::new().with_data_dir(dir.path().join("data"));
    Setup {
        api,
        device: device.to_string_lossy().into_owned(),
        mirror,
        _dir: dir,
    }
}

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

fn sources(mirror: &Path) -> Value {
    json!([
        {"id": "xlenore", "mirror": mirror.join("xlenore").to_string_lossy()},
        {"id": "libretro", "mirror": mirror.join("libretro").to_string_lossy()},
    ])
}

fn wait(api: &Api, started: &Value) -> yaoplm_api::jobs::JobInfo {
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    api.jobs().wait(job).unwrap()
}

fn execute(api: &Api, plan: &Value, confirm: bool) -> yaoplm_api::jobs::ExecutionReport {
    let started = call(
        api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": confirm}),
    )
    .unwrap();
    let info = wait(api, &started);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::Execution(report)) => report,
        other => panic!("unexpected result {other:?}"),
    }
}

fn scan_art(api: &Api, root: &str) -> Vec<(String, Vec<String>)> {
    let started = call(
        api,
        json!({"type": "ScanLibrary", "root": root, "verify": false}),
    )
    .unwrap();
    match wait(api, &started).result {
        Some(JobResult::Library(library)) => library
            .games
            .iter()
            .map(|g| {
                (
                    g.startup.clone(),
                    g.art
                        .iter()
                        .map(|a| {
                            format!(
                                "{} {}x{}",
                                a.kind,
                                a.width.unwrap_or(0),
                                a.height.unwrap_or(0)
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
        other => panic!("unexpected result {other:?}"),
    }
}

fn art_log(info: &yaoplm_api::jobs::JobInfo) -> Vec<String> {
    info.log
        .iter()
        .map(|entry| match entry {
            JobLogEntry::Art {
                startup,
                state,
                source,
                ..
            } => format!("{startup}:{state:?}:{}", source.as_deref().unwrap_or("-")),
            other => panic!("unexpected log entry {other:?}"),
        })
        .collect()
}

#[test]
fn fetch_missing_covers_then_write_them() {
    let s = setup();
    let started = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "sources": sources(&s.mirror)}),
    )
    .unwrap();
    assert_eq!(started["type"], "Job");
    let info = wait(&s.api, &started);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    assert_eq!(info.kind, yaoplm_api::jobs::JobKind::FetchArt);
    assert_eq!(info.total, Some(2), "Retail already has a cover");
    let log = art_log(&info);
    assert!(
        log.contains(&"SCES_500.03:Found:libretro".to_owned()),
        "{log:?}"
    );
    assert!(
        log.contains(&"SLUS_212.18:NotFound:-".to_owned()),
        "{log:?}"
    );
    assert_eq!(log.len(), 2);
    let Some(JobResult::Plan(plan)) = info.result else {
        panic!("a FetchArt job ends with a plan");
    };
    let plan = serde_json::to_value(plan).unwrap();
    assert_eq!(plan["kind"], "art");
    assert_eq!(plan["title"], "Write 1 cover");
    assert!(plan["executable"].as_bool().unwrap(), "{plan:#}");
    assert_eq!(plan["deletesUserData"], false);
    assert_eq!(plan["transactions"].as_array().unwrap().len(), 1);
    assert_eq!(plan["transactions"][0]["game"]["startup"], "SCES_500.03");
    assert_eq!(plan["transactions"][0]["steps"][0]["op"], "writeFile");
    assert_eq!(
        plan["transactions"][0]["steps"][0]["path"],
        "ART/SCES_500.03_COV.png"
    );
    assert!(
        plan["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "coversPresent"),
        "{plan:#}"
    );
    let skipped = plan["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["reason"]["code"], "artNotFound");

    let report = execute(&s.api, &plan, false);
    assert!(report.error.is_none(), "{report:?}");
    let art = scan_art(&s.api, &s.device);
    let lf = art.iter().find(|(id, _)| id == "SCES_500.03").unwrap();
    assert_eq!(lf.1, ["COV 140x200"]);
    let bytes = fs::read(Path::new(&s.device).join("ART/SCES_500.03_COV.png")).unwrap();
    assert_eq!(png_header(&bytes).unwrap().bit_depth, 8);

    // Nothing left to fetch: the job still ends with a plan, which says so.
    let started = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "sources": sources(&s.mirror)}),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let Some(JobResult::Plan(plan)) = info.result else {
        panic!("expected a plan");
    };
    assert!(!plan.executable);
    assert_eq!(plan.title, "No picture to write");
    assert!(
        plan.notices.iter().any(|n| n.code == "nothingToWrite"),
        "{plan:#?}"
    );
    assert_eq!(plan.skipped.len(), 1, "the DVD-9 game is still not found");
    assert_eq!(info.total, Some(1), "only the DVD-9 game is still missing");

    // With every game covered the job says so instead.
    let started = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "games": ["SLUS_200.62", "SCES_500.03"], "sources": sources(&s.mirror)}),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    assert_eq!(info.total, Some(0));
    let Some(JobResult::Plan(plan)) = info.result else {
        panic!("expected a plan");
    };
    assert!(
        plan.notices.iter().any(|n| n.code == "allHaveCovers"),
        "{plan:#?}"
    );
}

#[test]
fn overwrite_scope_replaces_covers_and_keeps_backups() {
    let s = setup();
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt", "root": s.device, "scope": "all",
            "games": ["SLUS-20062", "slus_212.18"],
            "sources": sources(&s.mirror),
            "profile": "RiptOpl"
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    assert_eq!(art_log(&info).len(), 2);
    let Some(JobResult::Plan(plan)) = info.result else {
        panic!("expected a plan");
    };
    let plan = serde_json::to_value(plan).unwrap();
    assert!(plan["executable"].as_bool().unwrap(), "{plan:#}");
    let txs = plan["transactions"].as_array().unwrap();
    // Folders (the backup folder) first, then the Retail cover.
    assert_eq!(txs.len(), 2, "{plan:#}");
    let retail = &txs[1];
    assert_eq!(retail["game"]["startup"], "SLUS_200.62");
    assert_eq!(retail["steps"][0]["op"], "rename");
    assert!(
        retail["steps"][0]["to"]
            .as_str()
            .unwrap()
            .starts_with(".yaoplm/backup/")
    );
    assert_eq!(retail["steps"][1]["op"], "writeFile");
    let report = execute(&s.api, &plan, false);
    assert!(report.error.is_none(), "{report:?}");
    let art = scan_art(&s.api, &s.device);
    let retail = art.iter().find(|(id, _)| id == "SLUS_200.62").unwrap();
    assert_eq!(
        retail.1,
        ["COV 280x400"],
        "RiptOPL covers are twice the size"
    );
    let backups: Vec<PathBuf> = walk(&Path::new(&s.device).join(".yaoplm/backup"));
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert!(backups[0].ends_with("ART/SLUS_200.62_COV.png"));
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
    }
    out
}

#[test]
fn set_art_from_a_file_and_from_bytes_delete_art() {
    let s = setup();
    let file = s.mirror.join("xlenore/covers/default/SLUS-20062.jpg");
    // A file for a game that has no cover yet.
    let plan = call(
        &s.api,
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_212.18",
            "input": {"kind": "file", "path": file.to_string_lossy()}
        }),
    )
    .unwrap();
    assert_eq!(plan["type"], "Plan");
    assert_eq!(plan["kind"], "art");
    assert!(plan["executable"].as_bool().unwrap(), "{plan:#}");
    assert_eq!(plan["transactions"].as_array().unwrap().len(), 1);
    let report = execute(&s.api, &plan, false);
    assert!(report.error.is_none(), "{report:?}");

    // Pasted bytes replace the Retail cover; the old file is backed up.
    let data = base64::engine::general_purpose::STANDARD.encode(image_bytes(
        90,
        120,
        image::ImageFormat::Png,
    ));
    let plan = call(
        &s.api,
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_200.62",
            "input": {"kind": "bytes", "data": data, "name": "clipboard"}
        }),
    )
    .unwrap();
    assert!(plan["executable"].as_bool().unwrap(), "{plan:#}");
    let tx = &plan["transactions"][1];
    assert_eq!(tx["steps"][0]["op"], "rename");
    assert!(
        tx["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "artReplaced")
    );
    let report = execute(&s.api, &plan, false);
    assert!(report.error.is_none(), "{report:?}");
    let art = scan_art(&s.api, &s.device);
    for id in ["SLUS_200.62", "SLUS_212.18"] {
        let game = art.iter().find(|(g, _)| g == id).unwrap();
        assert_eq!(game.1, ["COV 140x200"], "{id}");
    }

    // Not an image, no such game, a missing file.
    let err = call(
        &s.api,
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_200.62",
            "input": {"kind": "bytes", "data": base64::engine::general_purpose::STANDARD.encode(b"hello")}
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    let err = call(
        &s.api,
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_000.00",
            "input": {"kind": "file", "path": file.to_string_lossy()}
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = call(
        &s.api,
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_200.62",
            "input": {"kind": "file", "path": s.mirror.join("nope.png").to_string_lossy()}
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    // Deleting needs the confirmation and leaves a backup.
    let plan = call(
        &s.api,
        json!({"type": "DeleteArt", "root": s.device, "startup": "SLUS_212.18"}),
    )
    .unwrap();
    assert_eq!(plan["deletesUserData"], true);
    let refused = call(&s.api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    let report = execute(&s.api, &plan, true);
    assert!(report.error.is_none(), "{report:?}");
    let art = scan_art(&s.api, &s.device);
    assert!(
        art.iter()
            .find(|(g, _)| g == "SLUS_212.18")
            .unwrap()
            .1
            .is_empty()
    );
    let err = call(
        &s.api,
        json!({"type": "DeleteArt", "root": s.device, "startup": "SLUS_212.18"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[test]
fn source_settings_cache_and_attribution() {
    let s = setup();
    // A disabled xlenore and no libretro mirror: nothing is found offline
    // (the libretro source would go to the network, which the test must
    // not do, so it is disabled too).
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt", "root": s.device, "games": ["SLUS_200.62"], "scope": "all",
            "sources": [{"id": "xlenore", "enabled": false}, {"id": "libretro", "enabled": false}]
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    assert_eq!(art_log(&info), ["SLUS_200.62:NotFound:-"]);

    for bad in [
        json!([{"id": "oplmanager"}]),
        json!([{"id": "xlenore", "mirror": s.mirror.join("missing").to_string_lossy()}]),
    ] {
        let err = call(
            &s.api,
            json!({"type": "FetchArt", "root": s.device, "sources": bad}),
        )
        .unwrap_err();
        assert!(
            matches!(err.code, ErrorCode::BadRequest | ErrorCode::NotFound),
            "{err:?}"
        );
    }
    let err = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "kind": "LOGO2"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);

    // The cache fills from the mirrors and can be emptied.
    let started = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "sources": sources(&s.mirror)}),
    )
    .unwrap();
    wait(&s.api, &started);
    let cache = call(&s.api, json!({"type": "ArtCache"})).unwrap();
    assert_eq!(cache["type"], "ArtCache");
    assert!(cache["objects"].as_u64().unwrap() >= 1, "{cache}");
    assert!(cache["bytes"].as_u64().unwrap() > 0);
    assert!(cache["path"].as_str().unwrap().ends_with("data"));
    assert_eq!(
        call(&s.api, json!({"type": "ClearArtCache"})).unwrap()["type"],
        "Ack"
    );
    let cache = call(&s.api, json!({"type": "ArtCache"})).unwrap();
    assert_eq!(cache["objects"], 0);

    let about = call(&s.api, json!({"type": "Attribution"})).unwrap();
    assert_eq!(about["type"], "Attribution");
    assert_eq!(about["license"], "GPL-3.0-or-later");
    assert_eq!(about["version"], yaoplm_api::VERSION);
    let data = about["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["id"], "pcsx2");
    assert_eq!(data[0]["license"], "GPL-3.0");
    assert_eq!(data[1]["license"], "CC BY-SA 4.0");
    let art = about["art"].as_array().unwrap();
    assert_eq!(
        art.iter()
            .map(|a| a["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["xlenore", "libretro"]
    );
    assert!(art[0]["license"].is_null());
    assert!(about["titles"]["entries"].as_u64().unwrap() > 12_000);
    assert!(about["titles"]["built"].as_str().unwrap().starts_with("20"));
}

#[test]
fn the_default_data_dir_is_the_platform_folder_unless_overridden() {
    let api = Api::new();
    assert!(api.data_dir().ends_with("yaoplm"), "{:?}", api.data_dir());
    let custom = Api::new().with_data_dir("C:/custom");
    assert_eq!(custom.data_dir(), Path::new("C:/custom"));
}

/// Covers for games being added (ART-8): a search over an import plan,
/// then the import planned again with the cover set. One confirmation
/// writes the games and their covers.
#[test]
fn covers_found_for_an_import_land_with_the_games() {
    let s = setup();
    let incoming = s.mirror.parent().unwrap().join("incoming");
    fs::create_dir_all(&incoming).unwrap();
    // On the device already with a cover: nothing to search.
    fs::write(
        incoming.join("Retail.iso"),
        Variant::Retail.build().to_vec(),
    )
    .unwrap();
    // In the libretro mirror.
    fs::write(incoming.join("Doa.iso"), Variant::LfOnly.build().to_vec()).unwrap();
    // Nowhere.
    fs::write(incoming.join("Big.iso"), Variant::Dvd9.build().to_vec()).unwrap();
    let device = s.mirror.parent().unwrap().join("empty");
    fs::create_dir_all(device.join("ART")).unwrap();
    fs::write(
        device.join("ART/SLUS_200.62_COV.png"),
        image_bytes(140, 200, image::ImageFormat::Png),
    )
    .unwrap();
    let device = device.to_string_lossy().into_owned();
    let import = json!({
        "type": "PlanImport",
        "root": device,
        "sources": [incoming.to_string_lossy()],
    });
    let first = call(&s.api, import.clone()).unwrap();
    let search = wait(
        &s.api,
        &call(
            &s.api,
            json!({
                "type": "FetchArt",
                "root": device,
                "sources": sources(&s.mirror),
                "import": first["id"],
            }),
        )
        .unwrap(),
    );
    assert_eq!(search.status, JobStatus::Finished, "{search:?}");
    let Some(JobResult::Covers(found)) = search.result.clone() else {
        panic!("expected a cover set: {:?}", search.result);
    };
    assert_eq!((found.found, found.missing, found.present), (1, 1, 1));
    let lines = search
        .log
        .iter()
        .filter(|e| matches!(e, JobLogEntry::Art { .. }))
        .count();
    assert_eq!(lines, 2, "one line per game searched");

    let mut with_covers = import.clone();
    with_covers["covers"] = json!(found.id);
    let plan = call(&s.api, with_covers.clone()).unwrap();
    assert_eq!(plan["executable"], true, "{plan}");
    let writes: Vec<String> = plan["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|t| t["steps"].as_array().unwrap().clone())
        .filter(|s| s["op"] == "writeFile")
        .map(|s| s["path"].as_str().unwrap().to_owned())
        .collect();
    let doa = writes
        .iter()
        .position(|p| p == "CD/SCES_500.03.Doa.iso")
        .unwrap();
    assert_eq!(writes[doa + 1], "ART/SCES_500.03_COV.png", "{writes:?}");
    assert!(!writes.iter().any(|p| p == "ART/SLUS_200.62_COV.png"));
    // The cover nobody had is listed, the game is still added.
    let skipped = plan["skipped"].as_array().unwrap();
    assert!(
        skipped
            .iter()
            .any(|s| s["path"].as_str().unwrap().starts_with("SLUS_212.18")),
        "{skipped:?}"
    );
    // The set stays for the next re-plan (a title edited on the plan).
    let again = call(&s.api, with_covers).unwrap();
    assert_eq!(again["transactions"], plan["transactions"]);

    let report = execute(&s.api, &again, false);
    assert!(report.error.is_none(), "{report:?}");
    let cover = fs::read(Path::new(&device).join("ART/SCES_500.03_COV.png")).unwrap();
    let header = png_header(&cover).unwrap();
    assert_eq!((header.width, header.height), (140, 200));

    // Another device's plan, or one that is not an import, is refused.
    let err = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "import": again["id"]}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound, "{err:?}");
    let other = call(&s.api, import.clone()).unwrap();
    let err = call(
        &s.api,
        json!({"type": "FetchArt", "root": s.device, "import": other["id"]}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest, "{err:?}");
    let mut stale = import;
    stale["covers"] = json!("nope");
    let err = call(&s.api, stale).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

/// Picture types beyond the front cover (ART-2): several types in one
/// bulk run, the variants of one slot to pick from, a manual picture of
/// another type and its removal.
#[test]
fn other_picture_types_and_variants() {
    let s = setup();
    let libretro = s.mirror.join("libretro");
    for folder in ["Named_Snaps", "Named_Logos"] {
        fs::create_dir_all(libretro.join(folder)).unwrap();
        fs::write(
            libretro
                .join(folder)
                .join("Dead or Alive 2 (Europe) (En,Fr,De,Es,It).png"),
            image_bytes(320, 240, image::ImageFormat::Png),
        )
        .unwrap();
    }
    let threed = s.mirror.join("xlenore/covers/3d");
    fs::create_dir_all(&threed).unwrap();
    fs::write(
        threed.join("SLUS-20062.png"),
        image_bytes(250, 300, image::ImageFormat::Png),
    )
    .unwrap();

    // Screenshots, logos and icons for every game in one run.
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt",
            "root": s.device,
            "kinds": ["SCR", "LGO", "ICO"],
            "sources": sources(&s.mirror),
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let Some(JobResult::Plan(plan)) = info.result.clone() else {
        panic!("expected a plan: {info:?}");
    };
    let writes: Vec<String> = plan
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter(|s| matches!(s.op, yaoplm_api::plans::StepOp::WriteFile))
        .map(|s| s.path.clone())
        .collect();
    assert_eq!(
        writes,
        ["ART/SCES_500.03_SCR.png", "ART/SCES_500.03_LGO.png"],
        "{plan:#?}"
    );
    assert_eq!(plan.title, "Write 2 pictures");
    // One log line per game and type; icons have no source at all.
    let lines: Vec<(String, String)> = info
        .log
        .iter()
        .filter_map(|e| match e {
            JobLogEntry::Art {
                startup,
                art,
                detail,
                ..
            } => Some((
                format!("{startup} {art}"),
                detail.clone().unwrap_or_default(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 9, "{lines:?}");
    let icon = lines.iter().find(|(k, _)| k == "SLUS_200.62 ICO").unwrap();
    assert!(icon.1.contains("No open source"), "{icon:?}");
    assert!(
        plan.skipped.iter().any(|s| s.path.contains("(ICO)")),
        "{:?}",
        plan.skipped
    );
    let report = execute(&s.api, &serde_json::to_value(&*plan).unwrap(), false);
    assert!(report.error.is_none());
    let logo = fs::read(Path::new(&s.device).join("ART/SCES_500.03_LGO.png")).unwrap();
    let header = png_header(&logo).unwrap();
    assert_eq!(
        (header.width, header.height, header.color_type),
        (300, 125, 6)
    );
    let snap = fs::read(Path::new(&s.device).join("ART/SCES_500.03_SCR.png")).unwrap();
    assert_eq!(png_header(&snap).unwrap().width, 250);

    // The variants of one front cover: xlenore's 2-D and 3-D.
    let started = call(
        &s.api,
        json!({
            "type": "FindArt",
            "root": s.device,
            "startup": "SLUS_200.62",
            "kind": "COV",
            "sources": sources(&s.mirror),
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let Some(JobResult::ArtVariants(found)) = info.result else {
        panic!("expected variants: {info:?}");
    };
    assert_eq!(found.variants.len(), 2, "{:?}", found.variants);
    assert!(found.variants[1].origin.ends_with("SLUS-20062.png"));
    assert_eq!(
        (found.variants[0].width, found.variants[0].height),
        (140, 200)
    );
    // The chosen one goes back as bytes.
    let plan = call(
        &s.api,
        json!({
            "type": "SetArt",
            "root": s.device,
            "startup": "SLUS_200.62",
            "kind": "COV",
            "input": {"kind": "bytes", "data": found.variants[1].png, "name": "xlenore 3D"},
        }),
    )
    .unwrap();
    assert!(
        plan["notices"].to_string().contains("xlenore 3D")
            || plan["transactions"].to_string().contains("xlenore 3D")
    );
    // An icon nobody has: no variant, and the log says why.
    let started = call(
        &s.api,
        json!({"type": "FindArt", "root": s.device, "startup": "SLUS_200.62", "kind": "ICO"}),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let Some(JobResult::ArtVariants(none)) = info.result else {
        panic!("expected variants");
    };
    assert!(none.variants.is_empty());
    assert_eq!(info.log.len(), 1);

    // A manual background, then its removal, by type.
    let bg = image_bytes(800, 600, image::ImageFormat::Jpeg);
    let plan = call(
        &s.api,
        json!({
            "type": "SetArt",
            "root": s.device,
            "startup": "SLUS_212.18",
            "kind": "bg",
            "input": {"kind": "bytes", "data": base64::engine::general_purpose::STANDARD.encode(&bg)},
        }),
    )
    .unwrap();
    assert!(
        plan["transactions"]
            .to_string()
            .contains("ART/SLUS_212.18_BG.png")
    );
    execute(&s.api, &plan, false);
    let written = fs::read(Path::new(&s.device).join("ART/SLUS_212.18_BG.png")).unwrap();
    assert_eq!(png_header(&written).unwrap().width, 640);
    let plan = call(
        &s.api,
        json!({"type": "DeleteArt", "root": s.device, "startup": "SLUS_212.18", "kind": "BG"}),
    )
    .unwrap();
    assert_eq!(plan["title"], "Delete from SLUS_212.18: background");
    execute(&s.api, &plan, true);
    assert!(!Path::new(&s.device).join("ART/SLUS_212.18_BG.png").exists());
}
