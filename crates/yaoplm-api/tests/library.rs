//! ScanLibrary and ImageInfo through `dispatch`, as every front end calls them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobResult, JobStatus};
use yaoplm_api::{Api, ApiError, ErrorCode, Request, Response};
use yaoplm_test_support::Variant;

fn library_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("CD")).unwrap();
    fs::create_dir_all(dir.path().join("DVD")).unwrap();
    fs::write(
        dir.path().join("CD/Retail.iso"),
        Variant::Retail.build().to_vec(),
    )
    .unwrap();
    fs::write(
        dir.path().join("DVD/Ps1.iso"),
        Variant::Ps1.build().to_vec(),
    )
    .unwrap();
    dir
}

fn as_json(response: &Response) -> Value {
    serde_json::to_value(response).unwrap()
}

fn text(path: PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

/// Runs the scan job to its end; the library comes back as JSON.
fn scan(api: &Api, root: PathBuf) -> Result<Value, ApiError> {
    let Response::Job { job } = api.dispatch(Request::ScanLibrary {
        root: text(root),
        profile: None,
        verify: None,
        mode: None,
    })?
    else {
        panic!("ScanLibrary answers with a job");
    };
    let info = api.jobs().wait(job).unwrap();
    match (info.status, info.result) {
        (JobStatus::Finished, Some(JobResult::Library(library))) => {
            Ok(serde_json::to_value(library).unwrap())
        }
        _ => Err(info.error.unwrap()),
    }
}

fn info(api: &Api, path: PathBuf) -> Result<Response, ApiError> {
    api.dispatch(Request::ImageInfo {
        path: text(path),
        profile: None,
    })
}

#[test]
fn scan_library_over_json() {
    let dir = library_dir();
    let request = json!({"type": "ScanLibrary", "root": dir.path()}).to_string();
    let api = Api::new();
    let started = as_json(&api.dispatch_json(&request).unwrap());
    assert_eq!(started["type"], "Job");
    let job = serde_json::from_value(started["job"].clone()).unwrap();
    api.jobs().wait(job).unwrap();
    let state = as_json(&api.dispatch(Request::GetJob { job }).unwrap());
    assert_eq!(state["type"], "JobState");
    assert_eq!(state["status"], "finished");
    let response = &state["result"];
    assert_eq!(response["type"], "Library");
    assert_eq!(response["profile"], "Opl120Beta");
    assert_eq!(response["games"].as_array().unwrap().len(), 1);
    let game = &response["games"][0];
    assert_eq!(game["title"], "Retail");
    assert_eq!(game["startup"], "SLUS_200.62");
    assert_eq!(game["media"], "CD");
    assert_eq!(game["format"], "ISO");
    assert_eq!(game["nameFormat"], "new");
    assert_eq!(game["imageStartup"], "SLUS_200.62");
    let skipped = &response["skipped"][0];
    assert_eq!(skipped["reason"]["code"], "noStartup");
    assert!(skipped["path"].as_str().unwrap().ends_with("Ps1.iso"));
    // Paths are shown without Windows verbatim prefixes.
    assert!(!response["root"].as_str().unwrap().starts_with(r"\\?\"));
}

#[test]
fn image_info_over_json() {
    let dir = library_dir();
    let api = Api::new();
    let response = as_json(&info(&api, dir.path().join("CD/Retail.iso")).unwrap());
    assert_eq!(response["type"], "ImageInfo");
    assert_eq!(response["startup"], "SLUS_200.62");
    assert_eq!(response["hiddenReason"], Value::Null);
    assert_eq!(response["platform"], "PS2");
    assert_eq!(response["suggestedMedia"], "CD");
    assert_eq!(response["vmode"], "NTSC");
    assert_eq!(response["fileName"]["format"], "new");

    let response = as_json(&info(&api, dir.path().join("DVD/Ps1.iso")).unwrap());
    assert_eq!(response["platform"], "PS1");
    assert_eq!(response["startup"], Value::Null);
    assert!(response["hiddenReason"].as_str().unwrap().contains("BOOT2"));
}

#[test]
fn errors() {
    let dir = library_dir();
    let api = Api::new();
    assert_eq!(
        scan(&api, dir.path().join("nope")).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(
        scan(&api, dir.path().join("CD/Retail.iso"))
            .unwrap_err()
            .code,
        ErrorCode::BadRequest
    );
    assert_eq!(
        info(&api, dir.path().to_path_buf()).unwrap_err().code,
        ErrorCode::BadRequest
    );
}

#[test]
fn server_policy_limits_paths_to_roots() {
    let dir = library_dir();
    let root = fs::canonicalize(dir.path().join("CD")).unwrap();
    let api = Api::with_roots(vec![root]);
    assert!(scan(&api, dir.path().join("CD")).is_ok());
    assert!(info(&api, dir.path().join("CD/Retail.iso")).is_ok());
    // `..` cannot escape: paths are resolved before the check.
    let escaped = scan(&api, dir.path().join("CD").join("..").join("DVD"));
    assert_eq!(escaped.unwrap_err().code, ErrorCode::Forbidden);
    assert_eq!(
        scan(&api, dir.path().to_path_buf()).unwrap_err().code,
        ErrorCode::Forbidden
    );
}
