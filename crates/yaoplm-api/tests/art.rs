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
/// Alive 2 (SCES-50003) by its Redump name; the DVD-9 one is nowhere. The
/// mirror of OPL Manager's backup starts empty.
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

    fs::create_dir_all(mirror.join("oplm/PS2")).unwrap();

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

/// Every source from the mirrors, ScreenScraper off: no test goes out.
fn sources(mirror: &Path) -> Value {
    json!([
        {"id": "xlenore", "mirror": mirror.join("xlenore").to_string_lossy()},
        {"id": "oplm", "mirror": mirror.join("oplm").to_string_lossy()},
        {"id": "libretro", "mirror": mirror.join("libretro").to_string_lossy()},
        {"id": "screenscraper", "enabled": false},
    ])
}

/// Every source off.
fn no_sources() -> Value {
    json!([
        {"id": "xlenore", "enabled": false},
        {"id": "oplm", "enabled": false},
        {"id": "libretro", "enabled": false},
        {"id": "screenscraper", "enabled": false},
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
    assert_eq!(info.title, "Cover search");
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
    // Every source off: nothing is found, and nothing goes out.
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt", "root": s.device, "games": ["SLUS_200.62"], "scope": "all",
            "sources": no_sources()
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
        ["xlenore", "oplm", "libretro", "screenscraper"]
    );
    assert!(art[0]["license"].is_null());
    // Every type has a source; ScreenScraper needs this build's key.
    assert_eq!(art[0]["kinds"], json!(["COV"]));
    assert_eq!(art[1]["kinds"].as_array().unwrap().len(), 8);
    assert_eq!(art[3]["license"], "CC BY-NC-SA 4.0");
    assert_eq!(art[1]["available"], true);
    assert_eq!(
        art[3]["available"],
        std::env::var_os("YAOPLM_SCREENSCRAPER_DEVID").is_some()
    );
    assert!(about["titles"]["entries"].as_u64().unwrap() > 12_000);
    assert!(about["titles"]["built"].as_str().unwrap().starts_with("20"));
}

/// A cache that cannot be opened (here a file where its folder goes;
/// another process holding it too long) fails that call only: the next
/// one tries again rather than failing for the life of the process.
#[test]
fn an_art_cache_that_could_not_be_opened_is_tried_again() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    fs::write(&data, b"not a folder").unwrap();
    let api = Api::new().with_data_dir(&data);
    let err = call(&api, json!({"type": "ArtCache"})).unwrap_err();
    assert!(err.message.contains("cannot open the art cache"), "{err:?}");
    fs::remove_file(&data).unwrap();
    let cache = call(&api, json!({"type": "ArtCache"})).unwrap();
    assert_eq!(cache["objects"], 0, "{cache}");
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
    // Per game too, so the panel under the plan can count the games that
    // are still in it (walkthrough finding 4).
    let mut per_game: Vec<(String, u32, u32, u32)> = found
        .games
        .iter()
        .map(|g| (g.startup.clone(), g.found, g.missing, g.present))
        .collect();
    per_game.sort();
    assert_eq!(
        per_game,
        [
            ("SCES_500.03".to_owned(), 1, 0, 0),
            ("SLUS_200.62".to_owned(), 0, 0, 1),
            ("SLUS_212.18".to_owned(), 0, 1, 0),
        ]
    );
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

/// Every picture type for the games being added (ART-8 with ART-2): the
/// search over an import plan takes `kinds`, skips the pictures the device
/// has, and the plan writes each found picture right after its game. Only
/// missing covers are listed as skipped; the other types are counted.
#[test]
fn all_picture_types_for_an_import() {
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
    let incoming = s.mirror.parent().unwrap().join("incoming");
    fs::create_dir_all(&incoming).unwrap();
    fs::write(incoming.join("Doa.iso"), Variant::LfOnly.build().to_vec()).unwrap();
    fs::write(incoming.join("Big.iso"), Variant::Dvd9.build().to_vec()).unwrap();
    let device = s.mirror.parent().unwrap().join("empty");
    fs::create_dir_all(device.join("ART")).unwrap();
    // Dead or Alive 2 has its logo already: not searched again.
    fs::write(
        device.join("ART/SCES_500.03_LGO.png"),
        image_bytes(300, 125, image::ImageFormat::Png),
    )
    .unwrap();
    let device = device.to_string_lossy().into_owned();
    let import =
        json!({"type": "PlanImport", "root": device, "sources": [incoming.to_string_lossy()]});
    let first = call(&s.api, import.clone()).unwrap();
    let search = wait(
        &s.api,
        &call(
            &s.api,
            json!({
                "type": "FetchArt",
                "root": device,
                "sources": sources(&s.mirror),
                "kinds": ["COV", "SCR", "LGO"],
                "import": first["id"],
            }),
        )
        .unwrap(),
    );
    assert_eq!(search.title, "Picture search");
    let Some(JobResult::Covers(found)) = search.result.clone() else {
        panic!("expected a picture set: {:?}", search.result);
    };
    // Found: DOA2's cover and screenshot. Missing: three for the DVD-9 game.
    assert_eq!((found.found, found.missing, found.present), (2, 3, 1));
    let mut searched: Vec<String> = search
        .log
        .iter()
        .filter_map(|e| match e {
            JobLogEntry::Art { startup, art, .. } => Some(format!("{startup} {art}")),
            _ => None,
        })
        .collect();
    searched.sort();
    assert_eq!(
        searched,
        [
            "SCES_500.03 COV",
            "SCES_500.03 SCR",
            "SLUS_212.18 COV",
            "SLUS_212.18 LGO",
            "SLUS_212.18 SCR"
        ]
    );

    let mut with_art = import;
    with_art["covers"] = json!(found.id);
    let plan = call(&s.api, with_art).unwrap();
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
    assert_eq!(
        writes[doa + 1..doa + 3],
        ["ART/SCES_500.03_COV.png", "ART/SCES_500.03_SCR.png"],
        "{writes:?}"
    );
    let skipped: Vec<&str> = plan["skipped"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["path"].as_str().unwrap())
        .collect();
    assert_eq!(skipped.len(), 1, "only the missing cover: {skipped:?}");
    assert!(skipped[0].starts_with("SLUS_212.18"), "{skipped:?}");
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
    // OPL Manager's backup has Dead or Alive 2's disc icon.
    let oplm = s.mirror.join("oplm/PS2/SCES_500.03");
    fs::create_dir_all(&oplm).unwrap();
    fs::write(
        oplm.join("SCES_500.03_ICO.png"),
        image_bytes(64, 64, image::ImageFormat::Png),
    )
    .unwrap();

    // Screenshots, logos and icons for every game in one run; a type named
    // twice is searched once.
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt",
            "root": s.device,
            "kinds": ["SCR", "LGO", "ICO", "SCR"],
            "sources": sources(&s.mirror),
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    // Not covers only: a picture search.
    assert_eq!(info.title, "Picture search");
    assert_eq!(
        info.title_phrase.as_ref().unwrap().key,
        "titles.findPictures"
    );
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
        [
            "ART/SCES_500.03_SCR.png",
            "ART/SCES_500.03_LGO.png",
            "ART/SCES_500.03_ICO.png"
        ],
        "{plan:#?}"
    );
    assert_eq!(plan.title, "Write 3 pictures");
    // One log line per game and type; the other icons are not in the
    // backup.
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
    assert!(icon.1.contains("Not found in oplm"), "{icon:?}");
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
    // An icon no mirror has: no variant, and the log says why.
    let started = call(
        &s.api,
        json!({
            "type": "FindArt", "root": s.device, "startup": "SLUS_200.62", "kind": "ICO",
            "sources": sources(&s.mirror),
        }),
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

/// Picture sets (ART-5): an OPL Manager backup (zip), a folder and an
/// `art.tar`, read for the games on the device only.
#[test]
fn picture_sets_fill_the_games_on_the_device() {
    use yaoplm_test_support::packed;

    let s = setup();
    let sets = s.mirror.parent().unwrap().join("sets");
    fs::create_dir_all(sets.join("folder/ART")).unwrap();
    let jpg = |w, h| image_bytes(w, h, image::ImageFormat::Jpeg);
    let png = |w, h| image_bytes(w, h, image::ImageFormat::Png);
    // OPL Manager's layout: PS2/<ID>/<ID>_<TYPE>.jpg, numbered screenshots;
    // PS1 pictures come first, and an ID can be in both folders.
    let backup = packed::zip(
        &[
            ("PS1/SLUS_200.62/SLUS_200.62_ICO.png", &png(64, 64)),
            ("PS2/SLUS_200.62/SLUS_200.62_COV.jpg", &jpg(300, 420)),
            ("PS2/SLUS_200.62/SLUS_200.62_SCR_00.jpg", &jpg(640, 480)),
            ("PS2/SLUS_200.62/SLUS_200.62_SCR_01.jpg", &jpg(640, 480)),
            ("PS2/SLUS_200.62/SLUS_200.62_SCR_02.jpg", &jpg(640, 480)),
            ("PS2/SLUS_200.62/SLUS_200.62_BG_00.jpg", &jpg(800, 600)),
            ("PS2/SLUS_999.98/SLUS_999.98_COV.jpg", &jpg(300, 420)),
            ("PS2/SLUS_999.98/SLUS_999.98_ICO.png", &png(64, 64)),
            ("PS2_LIST.CSV", b"sep=,\nID,ID2,Title\n"),
        ],
        false,
    )
    .unwrap();
    fs::write(sets.join("OPLM_ART_2023_07.zip"), backup).unwrap();
    fs::write(sets.join("folder/ART/SCES_500.03_LGO.png"), png(400, 100)).unwrap();
    fs::write(sets.join("folder/notes.txt"), b"hello").unwrap();
    // Retail's cover again, in a second set: still one slot the device has.
    fs::write(sets.join("folder/ART/SLUS_200.62_COV.png"), png(140, 200)).unwrap();
    // A JPEG and a PNG of the same icon: the PNG wins.
    let art_tar = packed::tar(&[
        ("SLUS_212.18_ICO.jpg", &jpg(128, 128)),
        ("SLUS_212.18_ICO.png", &png(128, 128)),
    ]);
    fs::write(sets.join("art.tar"), art_tar).unwrap();

    let import = |overwrite: bool| {
        let started = call(
            &s.api,
            json!({
                "type": "ImportArt",
                "root": s.device,
                "sources": [
                    sets.join("OPLM_ART_2023_07.zip").to_string_lossy(),
                    sets.join("folder").to_string_lossy(),
                    sets.join("art.tar").to_string_lossy(),
                ],
                "overwrite": overwrite,
            }),
        )
        .unwrap();
        let info = wait(&s.api, &started);
        assert_eq!(info.status, JobStatus::Finished, "{info:?}");
        let Some(JobResult::Plan(plan)) = info.result else {
            panic!("expected a plan: {info:?}");
        };
        plan
    };
    let plan = import(false);
    let mut writes: Vec<String> = plan
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter(|s| matches!(s.op, yaoplm_api::plans::StepOp::WriteFile))
        .map(|s| s.path.clone())
        .collect();
    writes.sort();
    assert_eq!(
        writes,
        [
            "ART/SCES_500.03_LGO.png",
            "ART/SLUS_200.62_BG.png",
            "ART/SLUS_200.62_SCR.png",
            "ART/SLUS_200.62_SCR2.png",
            "ART/SLUS_212.18_ICO.png",
        ],
        "{plan:#?}"
    );
    // Retail's cover is on the device; the other game's two and the PS1
    // icon are not ours.
    let notice = |code: &str| {
        plan.notices
            .iter()
            .find(|n| n.code == code)
            .unwrap_or_else(|| panic!("no {code}: {:?}", plan.notices))
            .message
            .clone()
    };
    assert_eq!(
        notice("artPresent"),
        "Kept pictures the device already has: 1"
    );
    assert_eq!(
        notice("otherGamesLeftOut"),
        "Skipped pictures of games not on this device: 3"
    );
    let icon = plan
        .transactions
        .iter()
        .find(|t| t.steps.iter().any(|s| s.path == "ART/SLUS_212.18_ICO.png"))
        .unwrap();
    assert!(
        icon.notices
            .iter()
            .any(|n| n.message.contains("SLUS_212.18_ICO.png")),
        "the PNG of the two: {icon:?}"
    );
    execute(&s.api, &serde_json::to_value(&*plan).unwrap(), false);
    let size = |name: &str| {
        let bytes = fs::read(Path::new(&s.device).join("ART").join(name)).unwrap();
        let h = png_header(&bytes).unwrap();
        (h.width, h.height)
    };
    assert_eq!(size("SLUS_200.62_SCR2.png"), (250, 188));
    assert_eq!(size("SLUS_200.62_BG.png"), (640, 480));
    assert_eq!(size("SCES_500.03_LGO.png"), (300, 125));
    assert_eq!(size("SLUS_212.18_ICO.png"), (64, 64));

    // Replacing: the cover the device had moves to the backup folder.
    let plan = import(true);
    assert!(
        plan.transactions
            .iter()
            .any(|t| t.notices.iter().any(|n| n.code == "artReplaced")),
        "{plan:#?}"
    );

    // Not a set at all.
    let err = call(
        &s.api,
        json!({"type": "ImportArt", "root": s.device, "sources": [sets.join("folder/notes.txt").to_string_lossy()]}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

/// Keeps a file or folder from being opened while it lives: a handle that
/// shares nothing on Windows, no permissions on Unix. `false` when that
/// does not stop this process (root on Unix).
struct Locked {
    #[cfg(windows)]
    _handle: fs::File,
    #[cfg(unix)]
    path: PathBuf,
    works: bool,
}

impl Locked {
    fn new(path: &Path) -> Self {
        let works = |path: &Path| {
            if path.is_dir() {
                fs::read_dir(path).is_err()
            } else {
                fs::File::open(path).is_err()
            }
        };
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // FILE_FLAG_BACKUP_SEMANTICS: needed to open a folder.
            let handle = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .custom_flags(0x0200_0000)
                .open(path)
                .unwrap();
            Self {
                works: works(path),
                _handle: handle,
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            Self {
                works: works(path),
                path: path.to_path_buf(),
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Locked {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o755));
    }
}

/// Sets the compression method of the zip entry `name` to 0xBEEF, which no
/// zip tool assigns, in its local header and in the central directory.
fn unknown_zip_method(archive: &mut [u8], name: &str) {
    let at: Vec<usize> = archive
        .windows(name.len())
        .enumerate()
        .filter(|(_, w)| *w == name.as_bytes())
        .map(|(i, _)| i)
        .collect();
    let [local, central] = at[..] else {
        panic!("{name}: {at:?}");
    };
    // The name follows a 30-byte local header and a 46-byte central one.
    let local = local - 30;
    let central = central - 46;
    assert_eq!(archive[local..local + 4], *b"PK\x03\x04");
    assert_eq!(archive[central..central + 4], *b"PK\x01\x02");
    archive[local + 8..local + 10].copy_from_slice(&0xBEEFu16.to_le_bytes());
    archive[central + 10..central + 12].copy_from_slice(&0xBEEFu16.to_le_bytes());
}

/// A picture that cannot be read costs that picture, not the set: an
/// unreadable subfolder or file of a folder set, a zip member whose CRC is
/// wrong or whose compression method has no decoder. A file nobody wants
/// is not even opened. A tar or 7z whose stream is broken still fails as a
/// whole (ART-5, ADR-0033).
#[test]
fn unreadable_pictures_are_skipped_not_the_whole_set() {
    use yaoplm_test_support::packed;

    let s = setup();
    let sets = s.mirror.parent().unwrap().join("sets");
    fs::create_dir_all(sets.join("folder/locked")).unwrap();
    fs::create_dir_all(sets.join("folder/z")).unwrap();
    let png = |w, h| image_bytes(w, h, image::ImageFormat::Png);
    let icon = png(48, 48);
    let readme = b"no decoder for me either".as_slice();
    let mut backup = packed::zip(
        &[
            ("SLUS_200.62_SCR_00.png", &png(320, 240)),
            ("SLUS_212.18_ICO.png", &icon),
            ("SCES_500.03_ICO.png", &png(40, 40)),
            ("readme.txt", readme),
        ],
        true,
    )
    .unwrap();
    // Stored: the icon's bytes are in the archive as they are.
    let at = backup
        .windows(icon.len())
        .position(|w| w == icon.as_slice())
        .unwrap();
    backup[at + icon.len() / 2] ^= 0xFF;
    // A picture and a readme packed in a way there is no decoder for: the
    // picture is skipped with its path, the readme is never read.
    unknown_zip_method(&mut backup, "SCES_500.03_ICO.png");
    unknown_zip_method(&mut backup, "readme.txt");
    fs::write(sets.join("backup.zip"), backup).unwrap();
    fs::write(sets.join("folder/SLUS_212.18_BG.png"), png(640, 480)).unwrap();
    fs::write(sets.join("folder/SCES_500.03_LGO.png"), png(400, 100)).unwrap();
    fs::write(
        sets.join("folder/locked/SLUS_212.18_LGO.png"),
        png(400, 100),
    )
    .unwrap();
    // Files that cannot be opened and are not wanted either: a game that
    // is not on the device, a slot the device fills, a JPEG after the PNG
    // of the same slot (`z/` is read after the PNG).
    let unwanted = [
        "SLUS_999.98_COV.png",
        "SLUS_200.62_COV.png",
        "z/SLUS_212.18_BG.jpg",
    ];
    for name in unwanted {
        fs::write(sets.join("folder").join(name), png(64, 64)).unwrap();
    }
    let file_lock = Locked::new(&sets.join("folder/SCES_500.03_LGO.png"));
    let folder_lock = Locked::new(&sets.join("folder/locked"));
    let unwanted_locks: Vec<Locked> = unwanted
        .iter()
        .map(|name| Locked::new(&sets.join("folder").join(name)))
        .collect();

    let started = call(
        &s.api,
        json!({
            "type": "ImportArt",
            "root": s.device,
            "sources": [
                sets.join("backup.zip").to_string_lossy(),
                sets.join("folder").to_string_lossy(),
            ],
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    let Some(JobResult::Plan(plan)) = info.result else {
        panic!("expected a plan: {info:?}");
    };
    let mut writes: Vec<String> = plan
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter(|s| matches!(s.op, yaoplm_api::plans::StepOp::WriteFile))
        .map(|s| s.path.clone())
        .collect();
    writes.sort();
    let mut skipped: Vec<(String, String)> = plan
        .skipped
        .iter()
        .map(|s| (s.path.clone(), s.reason.code.clone()))
        .collect();
    skipped.sort();
    let mut expected_skips = vec![
        ("backup.zip: SCES_500.03_ICO.png", "pictureUnreadable"),
        ("backup.zip: SLUS_212.18_ICO.png", "pictureUnreadable"),
    ];
    let mut expected = vec!["ART/SLUS_200.62_SCR.png", "ART/SLUS_212.18_BG.png"];
    if file_lock.works {
        expected_skips.push(("folder: SCES_500.03_LGO.png", "pictureUnreadable"));
    } else {
        expected.insert(0, "ART/SCES_500.03_LGO.png");
    }
    if folder_lock.works {
        expected_skips.push(("folder: locked", "unreadable"));
    } else {
        expected.push("ART/SLUS_212.18_LGO.png");
    }
    expected_skips.sort();
    assert_eq!(
        skipped,
        expected_skips
            .iter()
            .map(|(path, code)| (path.to_string(), code.to_string()))
            .collect::<Vec<_>>(),
    );
    let method = plan
        .skipped
        .iter()
        .find(|s| s.path.ends_with("SCES_500.03_ICO.png"))
        .unwrap();
    assert!(
        method
            .reason
            .message
            .contains("unsupported compression method 48879"),
        "{method:?}"
    );
    assert_eq!(writes, expected, "{plan:#?}");
    // The unwanted ones were counted like any other, not opened.
    let notice = |code: &str| {
        plan.notices
            .iter()
            .find(|n| n.code == code)
            .map(|n| n.message.clone())
    };
    assert_eq!(
        notice("otherGamesLeftOut").as_deref(),
        Some("Skipped pictures of games not on this device: 1"),
        "{:?}",
        plan.notices
    );
    assert_eq!(
        notice("artPresent").as_deref(),
        Some("Kept pictures the device already has: 1"),
        "{:?}",
        plan.notices
    );
    assert!(
        unwanted_locks.iter().all(|l| l.works) || cfg!(unix),
        "the locks hold on Windows"
    );
    drop((file_lock, folder_lock, unwanted_locks));

    // A tar is one stream: when it breaks, the set cannot be read.
    let failed = |set: &Path| {
        let started = call(
            &s.api,
            json!({"type": "ImportArt", "root": s.device, "sources": [set.to_string_lossy()]}),
        )
        .unwrap();
        let info = wait(&s.api, &started);
        assert_eq!(info.status, JobStatus::Failed, "{info:?}");
        assert_eq!(
            info.error.unwrap().phrase.unwrap().key,
            "errors.pictureSetUnreadable"
        );
    };
    fs::write(sets.join("broken.tar"), vec![0x5A; 2048]).unwrap();
    failed(&sets.join("broken.tar"));

    // So is a solid 7z: a picture of a game on the device whose packed
    // bytes are damaged fails while it is read, and what follows it in the
    // block cannot be reached.
    let mut solid = packed::seven_zip(&[("SLUS_212.18_ICO.png", &png(48, 48))]).unwrap();
    // The packed streams run from the end of the 32-byte start header to
    // the next header, whose offset from there is at byte 12.
    let packed_len = u64::from_le_bytes(solid[12..20].try_into().unwrap()) as usize;
    solid[32 + packed_len / 2] ^= 0xFF;
    let mut read_failed = false;
    let walked = yaoplm_formats::archive::for_each_file(
        yaoplm_formats::archive::Kind::SevenZip,
        &mut Cursor::new(solid.clone()),
        &mut |_, data| {
            let read = data.read_to_end(&mut Vec::new()).map(drop);
            read_failed |= read.is_err();
            read
        },
    );
    assert!(walked.is_err() && read_failed, "{walked:?}");
    fs::write(sets.join("broken.7z"), solid).unwrap();
    failed(&sets.join("broken.7z"));
}

/// Cancelling a picture-set import ends the job without a plan, and
/// nothing is written (ART-5).
#[test]
fn cancelling_a_picture_set_import_leaves_no_plan() {
    let s = setup();
    let set = s.mirror.parent().unwrap().join("icons");
    fs::create_dir_all(&set).unwrap();
    for id in ["SCES_500.03", "SLUS_200.62", "SLUS_212.18"] {
        fs::write(
            set.join(format!("{id}_ICO.png")),
            image_bytes(48, 48, image::ImageFormat::Png),
        )
        .unwrap();
    }
    // Cancelled as soon as the first picture is taken: listeners run on
    // the job's thread, so the next picture sees it.
    let jobs = s.api.jobs().clone();
    let _cancel = s.api.subscribe(move |event| {
        if let yaoplm_api::JobEvent::Log {
            job,
            entry: JobLogEntry::Art { .. },
        } = event
        {
            let _ = jobs.cancel(*job);
        }
    });
    let started = call(
        &s.api,
        json!({"type": "ImportArt", "root": s.device, "sources": [set.to_string_lossy()]}),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    assert_eq!(info.status, JobStatus::Cancelled, "{info:?}");
    assert!(info.result.is_none(), "no plan: {:?}", info.result);
    let found = info
        .log
        .iter()
        .filter(|e| matches!(e, JobLogEntry::Art { .. }))
        .count();
    assert_eq!(found, 1, "{:?}", info.log);
    assert!(
        !Path::new(&s.device)
            .join("ART/SCES_500.03_ICO.png")
            .exists()
    );
}

/// A game the bundled database does not know has no Redump name, which
/// libretro looks pictures up by: the log says so instead of claiming no
/// source has the type (ART-2).
#[test]
fn a_game_without_a_redump_name_says_why_nothing_was_found() {
    use yaoplm_test_support::fixtures;

    let s = setup();
    fs::write(
        Path::new(&s.device).join("DVD/SLPM_999.99.Homebrew.iso"),
        fixtures::boot_iso(fixtures::placeholder_elf(), "SLPM_999.99", 0)
            .build()
            .to_vec(),
    )
    .unwrap();
    let libretro_only = json!([
        {"id": "xlenore", "enabled": false},
        {"id": "oplm", "enabled": false},
        {"id": "libretro", "mirror": s.mirror.join("libretro").to_string_lossy()},
        {"id": "screenscraper", "enabled": false},
    ]);
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt",
            "root": s.device,
            "games": ["SLPM_999.99"],
            "kinds": ["SCR"],
            "sources": libretro_only,
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let Some(JobResult::Plan(plan)) = info.result.clone() else {
        panic!("expected a plan: {info:?}");
    };
    assert_eq!(plan.skipped.len(), 1, "{:?}", plan.skipped);
    assert_eq!(plan.skipped[0].reason.code, "notInTitleDatabase");
    let JobLogEntry::Art { detail_phrase, .. } = &info.log[0] else {
        panic!("unexpected log {:?}", info.log);
    };
    assert_eq!(
        detail_phrase.as_ref().unwrap().key,
        "notices.notInTitleDatabase"
    );

    // The same for one slot's variants.
    let started = call(
        &s.api,
        json!({
            "type": "FindArt",
            "root": s.device,
            "startup": "SLPM_999.99",
            "kind": "SCR",
            "sources": libretro_only,
        }),
    )
    .unwrap();
    let info = wait(&s.api, &started);
    let JobLogEntry::Art { detail, .. } = &info.log[0] else {
        panic!("unexpected log {:?}", info.log);
    };
    assert!(
        detail.as_deref().unwrap().contains("no Redump name"),
        "{detail:?}"
    );

    // With every source off, that is the reason.
    let started = call(
        &s.api,
        json!({
            "type": "FetchArt",
            "root": s.device,
            "games": ["SLPM_999.99"],
            "kinds": ["SCR"],
            "sources": no_sources(),
        }),
    )
    .unwrap();
    let Some(JobResult::Plan(plan)) = wait(&s.api, &started).result else {
        panic!("expected a plan");
    };
    assert_eq!(plan.skipped[0].reason.code, "noArtSource");
}

/// The server reads picture sets, mirrors and picture files only inside
/// its roots, as it does sources of games (ADR-0009).
#[test]
fn a_server_reads_sets_and_mirrors_only_inside_its_roots() {
    let s = setup();
    let root = fs::canonicalize(&s.device).unwrap();
    let server = Api::with_roots(vec![root]).with_data_dir(s.mirror.parent().unwrap().join("data"));
    let outside = s.mirror.join("xlenore");
    let mirror = json!([{"id": "xlenore", "mirror": outside.to_string_lossy()}]);
    let refused = [
        json!({"type": "ImportArt", "root": s.device, "sources": [outside.to_string_lossy()]}),
        json!({"type": "FetchArt", "root": s.device, "sources": mirror}),
        json!({"type": "FindArt", "root": s.device, "startup": "SLUS_200.62", "kind": "COV", "sources": mirror}),
        json!({
            "type": "SetArt", "root": s.device, "startup": "SLUS_200.62",
            "input": {"kind": "file", "path": outside.join("covers/default/SLUS-20062.jpg").to_string_lossy()},
        }),
    ];
    for request in refused {
        let err = call(&server, request.clone()).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden, "{request}: {err:?}");
    }
    // A set inside a root is read.
    let inside = Path::new(&s.device).join("set");
    fs::create_dir_all(&inside).unwrap();
    fs::write(
        inside.join("SLUS_212.18_ICO.png"),
        image_bytes(48, 48, image::ImageFormat::Png),
    )
    .unwrap();
    let started = call(
        &server,
        json!({"type": "ImportArt", "root": s.device, "sources": [inside.to_string_lossy()]}),
    )
    .unwrap();
    let info = wait(&server, &started);
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
}
