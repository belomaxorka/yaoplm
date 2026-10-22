//! Plan → confirm → execute through `dispatch`, on a temp folder as the
//! device (IMP-10, GAME-1 to GAME-3, DEV-1, DEV-2).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobLogEntry, JobResult, JobStatus, StepLogState, TxLogState, TxOutcome};
use yaoplm_api::{Api, ErrorCode, JobEvent, JobId, Request, Response};
use yaoplm_test_support::Variant;
use yaoplm_test_support::bincue::{RawMode, cue_sheet, to_bin};

struct Setup {
    _dir: tempfile::TempDir,
    device: String,
    sources: std::path::PathBuf,
}

fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    let sources = dir.path().join("games");
    fs::create_dir_all(&device).unwrap();
    fs::create_dir_all(&sources).unwrap();
    fs::write(sources.join("Retail.iso"), Variant::Retail.build().to_vec()).unwrap();
    let iso = Variant::LfOnly.build().to_vec();
    fs::write(
        sources.join("Lf Game.bin"),
        to_bin(&iso, RawMode::Mode2_2352, 0),
    )
    .unwrap();
    fs::write(
        sources.join("Lf Game.cue"),
        cue_sheet("Lf Game.bin", RawMode::Mode2_2352, 0),
    )
    .unwrap();
    Setup {
        device: device.to_string_lossy().into_owned(),
        sources,
        _dir: dir,
    }
}

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

/// Executes a plan and waits for the job.
fn execute(api: &Api, plan: &Value, confirm: bool) -> yaoplm_api::jobs::ExecutionReport {
    let started = call(
        api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": confirm}),
    )
    .unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::Execution(report)) => report,
        other => panic!("unexpected result {other:?}"),
    }
}

fn scan(api: &Api, root: &str) -> Value {
    let started = call(api, json!({"type": "ScanLibrary", "root": root})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    match api.jobs().wait(job).unwrap().result {
        Some(JobResult::Library(library)) => serde_json::to_value(library).unwrap(),
        other => panic!("unexpected result {other:?}"),
    }
}

fn titles(library: &Value) -> Vec<String> {
    library["games"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            format!(
                "{} {}",
                g["media"].as_str().unwrap(),
                g["title"].as_str().unwrap()
            )
        })
        .collect()
}

#[test]
fn import_rename_move_delete() {
    let s = setup();
    let api = Api::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    let _sub = api.subscribe(move |e: &JobEvent| {
        let name = serde_json::to_value(e).unwrap()["type"]
            .as_str()
            .unwrap()
            .to_owned();
        log.lock().unwrap().push(name);
    });

    let plan = call(
        &api,
        json!({"type": "PlanImport", "root": s.device, "sources": [s.sources]}),
    )
    .unwrap();
    assert_eq!(plan["type"], "Plan");
    assert_eq!(plan["kind"], "import");
    assert_eq!(plan["executable"], true);
    assert_eq!(plan["deletesUserData"], false);
    let txs = plan["transactions"].as_array().unwrap();
    assert_eq!(txs.len(), 3, "{plan:#}");
    assert_eq!(txs[1]["steps"][0]["op"], "writeFile");
    assert_eq!(txs[1]["commit"], 1);
    let report = execute(&api, &plan, false);
    assert_eq!(report.outcomes, [TxOutcome::Done; 3]);
    assert!(events.lock().unwrap().iter().any(|e| e == "Progress"));
    assert_eq!(
        events.lock().unwrap().last().map(String::as_str),
        Some("Finished")
    );
    // A plan runs once.
    let again = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap_err();
    assert_eq!(again.code, ErrorCode::NotFound);

    let library = scan(&api, &s.device);
    assert_eq!(titles(&library), ["CD Lf Game", "CD Retail"]);

    // Rename by the path the scan reported.
    let retail = library["games"][1]["path"].as_str().unwrap().to_owned();
    let plan = call(
        &api,
        json!({"type": "RenameGame", "root": s.device, "game": {"kind": "image", "path": retail}, "title": "Retail Renamed"}),
    )
    .unwrap();
    assert_eq!(execute(&api, &plan, false).outcomes, [TxOutcome::Done]);

    // Move by a path relative to the root.
    let plan = call(
        &api,
        json!({"type": "MoveGame", "root": s.device, "game": {"kind": "image", "path": "CD/SCES_500.03.Lf Game.iso"}, "media": "DVD"}),
    )
    .unwrap();
    execute(&api, &plan, false);
    assert_eq!(
        titles(&scan(&api, &s.device)),
        ["DVD Lf Game", "CD Retail Renamed"]
    );

    // Deleting needs an explicit confirmation; the plan survives a refusal.
    let plan = call(
        &api,
        json!({"type": "DeleteGame", "root": s.device, "game": {"kind": "image", "path": "DVD/SCES_500.03.Lf Game.iso"}}),
    )
    .unwrap();
    assert_eq!(plan["deletesUserData"], true);
    let refused = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap_err();
    assert_eq!(refused.code, ErrorCode::BadRequest);
    execute(&api, &plan, true);
    assert_eq!(titles(&scan(&api, &s.device)), ["CD Retail Renamed"]);
    // Sources were copied, never touched.
    assert!(s.sources.join("Retail.iso").exists());
}

#[test]
fn ul_games_by_index() {
    let s = setup();
    let api = Api::new();
    let plan = call(
        &api,
        json!({"type": "PlanImport", "root": s.device, "sources": [s.sources.join("Retail.iso")], "layout": "ul"}),
    )
    .unwrap();
    execute(&api, &plan, false);
    let library = scan(&api, &s.device);
    let game = &library["games"][0];
    assert_eq!(game["format"], "UL");
    assert_eq!(game["ulIndex"], 0);
    let plan = call(
        &api,
        json!({"type": "RenameGame", "root": s.device, "game": {"kind": "ul", "index": 0, "startup": "SLUS_200.62", "name": "Retail"}, "title": "Short"}),
    )
    .unwrap();
    execute(&api, &plan, false);
    let library = scan(&api, &s.device);
    assert_eq!(library["games"][0]["title"], "Short");
    assert!(
        library["games"][0]["warnings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn devices_and_server_policy() {
    let s = setup();
    let api = Api::new();
    let devices = call(&api, json!({"type": "ListDevices"})).unwrap();
    assert!(!devices["devices"].as_array().unwrap().is_empty());
    let device = call(&api, json!({"type": "InspectDevice", "root": s.device})).unwrap();
    assert_eq!(device["type"], "Device");
    assert_eq!(device["appleDouble"], 0);
    assert!(device["advice"].is_array());

    // The server only reads and deletes inside its roots.
    let root = fs::canonicalize(Path::new(&s.device)).unwrap();
    let server = Api::with_roots(vec![root.clone()]);
    let err = call(
        &server,
        json!({"type": "PlanImport", "root": s.device, "sources": [s.sources]}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let devices = call(&server, json!({"type": "ListDevices"})).unwrap();
    let listed = devices["devices"].as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0]["root"], yaoplm_api::display_path(&root));
}

#[test]
fn recovery_plans_through_the_api() {
    let s = setup();
    let api = Api::new();
    let plan = call(&api, json!({"type": "PlanRecovery", "root": s.device})).unwrap();
    assert_eq!(plan["kind"], "recovery");
    assert_eq!(plan["executable"], false);
    assert_eq!(plan["notices"][0]["code"], "nothingToRecover");
    let init = call(&api, json!({"type": "PlanInit", "root": s.device})).unwrap();
    assert_eq!(
        init["transactions"][0]["steps"].as_array().unwrap().len(),
        9
    );
    execute(&api, &init, false);
    assert!(Path::new(&s.device).join("APPS").is_dir());
    let Response::Jobs { jobs } = api.dispatch(Request::ListJobs).unwrap() else {
        panic!()
    };
    assert_eq!(jobs.len(), 1);
}

#[test]
fn job_log_reports_transactions_and_steps() {
    let s = setup();
    let api = Api::new();
    let events = Arc::new(Mutex::new(Vec::<JobEvent>::new()));
    let log = Arc::clone(&events);
    let _sub = api.subscribe(move |e: &JobEvent| log.lock().unwrap().push(e.clone()));

    let plan = call(
        &api,
        json!({"type": "PlanImport", "root": s.device, "sources": [s.sources.join("Retail.iso")]}),
    )
    .unwrap();
    let started = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished);
    assert_eq!(info.root.as_deref(), plan["root"].as_str());

    // Every transaction is logged as started and done, with the plan's own
    // title, and every step as started and then done.
    let txs = plan["transactions"].as_array().unwrap();
    let states: Vec<String> = info
        .log
        .iter()
        .map(|entry| match entry {
            JobLogEntry::Transaction {
                state,
                title,
                index,
                ..
            } => {
                assert_eq!(Some(title.as_str()), txs[*index as usize]["title"].as_str());
                format!("tx{index}:{state:?}")
            }
            JobLogEntry::Step {
                op, state, path, ..
            } => format!("{op:?}:{state:?}:{path}"),
        })
        .collect();
    assert_eq!(states.first().map(String::as_str), Some("tx0:Started"));
    assert_eq!(
        states.last().map(String::as_str),
        Some(format!("tx{}:Done", txs.len() - 1).as_str())
    );
    let write_started = states
        .iter()
        .position(|s| s.starts_with("WriteFile:Started:CD/"))
        .expect("the write step is logged");
    assert_eq!(
        states[write_started + 1],
        states[write_started].replace("Started", "Done")
    );
    assert!(states[1].starts_with("CreateDir:Started:"), "{states:?}");
    let steps: usize = txs
        .iter()
        .map(|tx| tx["steps"].as_array().unwrap().len())
        .sum();
    assert_eq!(states.len(), 2 * txs.len() + 2 * steps);
    let published = events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, JobEvent::Log { job: id, .. } if *id == job))
        .count();
    assert_eq!(published, info.log.len());
    assert!(
        events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, JobEvent::Queued { job: id, root: Some(_), .. } if *id == job))
    );

    // ListJobs leaves the log out; GetJob has it.
    let Response::Jobs { jobs } = api.dispatch(Request::ListJobs).unwrap() else {
        panic!()
    };
    assert!(jobs.iter().all(|j| j.log.is_empty()));
    let Response::JobState(state) = api.dispatch(Request::GetJob { job }).unwrap() else {
        panic!()
    };
    assert_eq!(state.log.len(), states.len());

    // A failed step is logged with its error, and the transaction rolls back.
    let library = scan(&api, &s.device);
    let retail = library["games"][0]["path"].as_str().unwrap().to_owned();
    let plan = call(
        &api,
        json!({"type": "RenameGame", "root": s.device, "game": {"kind": "image", "path": retail}, "title": "Taken"}),
    )
    .unwrap();
    fs::write(
        Path::new(&s.device).join("CD/SLUS_200.62.Taken.iso"),
        b"someone else's file",
    )
    .unwrap();
    let started = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    let Some(JobResult::Execution(report)) = &info.result else {
        panic!("{info:?}");
    };
    assert_eq!(report.outcomes, [TxOutcome::RolledBack]);
    assert!(report.error.is_some());
    let failed = info.log.iter().find_map(|entry| match entry {
        JobLogEntry::Step {
            state: StepLogState::Failed,
            error,
            ..
        } => error.clone(),
        _ => None,
    });
    assert!(
        failed
            .as_deref()
            .is_some_and(|e| e.contains("already exists")),
        "{:?}",
        info.log
    );
    assert!(matches!(
        info.log.last(),
        Some(JobLogEntry::Transaction {
            state: TxLogState::RolledBack,
            ..
        })
    ));
}
