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
    assert_eq!(skipped[0]["reason"]["code"], "coverNotFound");

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
    assert_eq!(plan.title, "No cover to write");
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
            .any(|n| n["code"] == "coverReplaced")
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
        json!({"type": "FetchArt", "root": s.device, "kind": "ICO"}),
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
