//! Conversions and compressed imports through `dispatch` on a real folder
//! (GAME-4): the plan, the delete confirmation, the files that result.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::Path;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobResult, JobStatus};
use yaoplm_api::{Api, ErrorCode, JobId};
use yaoplm_formats::image::DiscImage;
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_test_support::Variant;

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

fn execute(api: &Api, plan: &Value) {
    let started = call(
        api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": true}),
    )
    .unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    let Some(JobResult::Execution(report)) = info.result else {
        panic!("unexpected result {:?}", info.result);
    };
    assert!(report.error.is_none(), "{report:?}");
}

fn iso_of(path: &Path) -> Vec<u8> {
    let mut image = DiscImage::open(Cursor::new(fs::read(path).unwrap())).unwrap();
    let mut out = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    for lba in 0..image.sector_count() {
        image.read_sector(lba, &mut sector).unwrap();
        out.extend_from_slice(&sector);
    }
    out
}

#[test]
fn convert_to_zso_and_back_on_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    fs::create_dir_all(device.join("CD")).unwrap();
    let iso = Variant::Retail.build().to_vec();
    fs::write(device.join("CD/SLUS_200.62.Retail.iso"), &iso).unwrap();
    let api = Api::new().with_data_dir(dir.path().join("data"));
    let root = device.to_string_lossy();

    let plan = call(
        &api,
        json!({
            "type": "ConvertGames",
            "root": root,
            "games": [{"kind": "image", "path": "CD/SLUS_200.62.Retail.iso"}],
            "target": "zso",
        }),
    )
    .unwrap();
    assert_eq!(plan["kind"], "convert");
    assert_eq!(plan["deletesUserData"], true);
    let steps = &plan["transactions"][0]["steps"];
    assert_eq!(steps[0]["op"], "writeZso");
    assert_eq!(steps[1]["op"], "remove");
    assert_eq!(steps[1]["removal"], "userData");

    // Deleting the ISO needs the explicit confirmation.
    let refused = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    execute(&api, &plan);
    let zso = device.join("CD/SLUS_200.62.Retail.zso");
    assert!(!device.join("CD/SLUS_200.62.Retail.iso").exists());
    assert_eq!(iso_of(&zso), iso);

    let back = call(
        &api,
        json!({
            "type": "ConvertGames",
            "root": root,
            "games": [{"kind": "image", "path": zso.to_string_lossy()}],
            "target": "iso",
        }),
    )
    .unwrap();
    execute(&api, &back);
    assert_eq!(
        fs::read(device.join("CD/SLUS_200.62.Retail.iso")).unwrap(),
        iso
    );
    assert!(!zso.exists());
}

#[test]
fn import_compressed() {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    fs::create_dir_all(&device).unwrap();
    let sources = dir.path().join("games");
    fs::create_dir_all(&sources).unwrap();
    let iso = Variant::Dvd9.build().to_vec();
    fs::write(sources.join("Dvd9 Game.iso"), &iso).unwrap();
    let api = Api::new().with_data_dir(dir.path().join("data"));

    let plan = call(
        &api,
        json!({
            "type": "PlanImport",
            "root": device.to_string_lossy(),
            "sources": [sources.join("Dvd9 Game.iso").to_string_lossy()],
            "compress": true,
        }),
    )
    .unwrap();
    let tx = &plan["transactions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| !t["game"].is_null())
        .cloned()
        .unwrap();
    assert_eq!(tx["game"]["format"], "ZSO");
    assert!(
        tx["notices"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "compressedToZso")
    );
    execute(&api, &plan);
    let written = device.join("DVD/SLUS_212.18.Dvd9 Game.zso");
    assert_eq!(iso_of(&written), iso);
    // The source stays: compressing is not moving.
    assert!(sources.join("Dvd9 Game.iso").exists());
}
