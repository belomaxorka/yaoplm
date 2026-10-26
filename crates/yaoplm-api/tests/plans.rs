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
fn titles_and_errors_carry_phrases() {
    let s = setup();
    let api = Api::new();
    let plan = call(
        &api,
        json!({"type": "PlanImport", "root": s.device, "sources": [s.sources]}),
    )
    .unwrap();
    // ADR-0029: the English title comes from the catalogue the UI
    // translates the phrase with.
    assert_eq!(plan["title"], "Import 2 games");
    assert_eq!(
        plan["titlePhrase"],
        json!({"key": "titles.importGames", "params": {"count": 2}})
    );
    let copy = &plan["transactions"][1];
    assert_eq!(copy["titlePhrase"]["key"], "titles.copyGame");
    let name = copy["titlePhrase"]["params"]["name"].as_str().unwrap();
    assert_eq!(copy["title"], format!("Copy {name} to CD"));

    let err = call(&api, json!({"type": "ExecutePlan", "plan": "gone"})).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(err.message, "Plan gone expired or already ran");
    let phrase = err.phrase.unwrap();
    assert_eq!(phrase.key, "errors.planExpired");
    assert_eq!(
        serde_json::to_value(&phrase.params).unwrap(),
        json!({"id": "gone"})
    );
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
fn the_loader_profile_of_a_device() {
    let s = setup();
    let api = Api::new();
    // As a stick plugged into the console: in a folder read over SMB the
    // reasons add that the loader's files are copies.
    let inspect = |extra: Value| {
        let mut request = json!({"type": "InspectDevice", "root": s.device, "mode": "ps2"});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        call(&api, request).unwrap()["loader"].clone()
    };

    // Nothing on the device: the default, and the reason says so.
    let loader = inspect(json!({}));
    assert_eq!(loader["profile"], "Opl120Beta");
    assert_eq!(loader["source"], "detected");
    assert_eq!(loader["detected"]["confidence"], "default");
    assert_eq!(
        loader["detected"]["reason"]["key"],
        "reasons.profileNoSigns"
    );
    // A folder on a fixed disk is keyed by its path.
    let key = loader["key"].as_str().unwrap().to_owned();
    assert!(key.starts_with("path:"), "{key}");

    // RiptOPL's settings next to its app.
    let app = Path::new(&s.device).join("APPS").join("APP_RIPTOPL");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("settings_riptopl.cfg"), "enable_usb=1\r\n").unwrap();
    let loader = inspect(json!({}));
    assert_eq!(loader["profile"], "RiptOpl");
    assert_eq!(loader["detected"]["confidence"], "indirect");
    assert_eq!(
        loader["detected"]["reasonText"],
        "APPS/APP_RIPTOPL/settings_riptopl.cfg, which only RiptOPL writes"
    );

    // A profile forced by the request (the CLI's --profile) beats the files,
    // an override beats both.
    let loader = inspect(json!({"profile": "Opl110"}));
    assert_eq!(
        (&loader["profile"], &loader["source"]),
        (&json!("Opl110"), &json!("explicit"))
    );
    let loader = inspect(json!({
        "profile": "Opl110",
        "overrides": [{"key": key, "profile": "Opl120Beta"}, {"key": "serial:0", "profile": "RiptOpl"}],
    }));
    assert_eq!(
        (&loader["profile"], &loader["source"]),
        (&json!("Opl120Beta"), &json!("override"))
    );
    // The detection is still there to show.
    assert_eq!(loader["detected"]["profile"], "RiptOpl");

    // The doctor speaks for the profile in force.
    let device = call(
        &api,
        json!({"type": "InspectDevice", "root": s.device, "profile": "Opl110"}),
    )
    .unwrap();
    let devices = call(
        &api,
        json!({"type": "ListDevices", "overrides": [{"key": key, "profile": "Opl110"}]}),
    )
    .unwrap();
    assert!(device["advice"].is_array());
    assert!(
        devices["devices"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["loader"]["key"].is_string())
    );
}

#[test]
fn a_memory_card_emulator_s_sd_is_known_by_its_files() {
    // ADR-0048: the files decide, even in a temp folder on the computer's
    // disk (a copy of the SD), and they stay protected in any mode.
    let s = setup();
    let device = Path::new(&s.device);
    yaoplm_test_support::emulator::write_to(device).unwrap();
    let api = Api::new();
    let inspect = |extra: Value| {
        let mut request = json!({"type": "InspectDevice", "root": s.device});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        call(&api, request).unwrap()
    };
    let codes = |device: &Value| -> Vec<String> {
        device["advice"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["code"].as_str().unwrap().to_owned())
            .collect()
    };

    let info = inspect(json!({}));
    assert_eq!(
        (
            &info["mode"]["mode"],
            &info["mode"]["source"],
            &info["mode"]["detected"]
        ),
        (&json!("emulator"), &json!("detected"), &json!("emulator"))
    );
    assert_eq!(
        info["mode"]["placeText"],
        "the files of a memory card emulator: .sd2psx, MemoryCards"
    );
    let sd = &info["emulator"];
    assert_eq!(sd["kinds"], json!(["sd2psx"]));
    assert_eq!(sd["folder"], "");
    assert_eq!(
        sd["protected"],
        json!([".sd2psx", "MemoryCards", "civ.bin"])
    );
    assert_eq!(sd["variants"][0]["name"], "PS2");
    assert_eq!(sd["variants"][0]["boot"][1]["name"], "PS2BBL");
    assert_eq!(sd["variants"][0]["gameCards"], 1);
    assert_eq!(sd["settings"][2]["name"], "PS2");
    assert_eq!(
        sd["settings"][2]["entries"][2],
        json!({"key": "CardSize", "value": "64"})
    );
    // A copy on the computer: no rule of the SD, but its card size; the
    // warning first, the note (information) after it.
    assert_eq!(codes(&info), ["emulatorCopy", "emulatorCardSize"]);
    // What the boot cards hold, from their own file systems.
    let one = &sd["variants"][0]["boot"][0]["contents"];
    assert_eq!(one["launchers"], json!(["FreeMcBoot"]));
    assert_eq!(one["loaders"][0]["version"], "v1.2.0-Beta-2216-bb415d2");
    assert_eq!(one["loaders"][0]["mmce"], true);
    assert_eq!(
        sd["variants"][0]["boot"][1]["contents"]["launchers"],
        json!(["PS2BBL"])
    );
    // The MMCE build in boot card 1 answers which OPL reads the SD.
    assert_eq!(info["loader"]["detected"]["confidence"], "indirect");
    assert_eq!(
        info["loader"]["detected"]["reasonText"],
        "v1.2.0-Beta-2216-bb415d2 with MMCE in APPS/OPL-MMCE-BETA2.ELF inside boot card 1 (PS2)"
    );
    assert_eq!(info["askProfile"], false);
    // Without one it is asked, even without OPL's folders: the answer
    // decides whether they come.
    let card1 = device.join("MemoryCards/PS2/BOOT/BootCard-1.mcd");
    let with_mmce = fs::read(&card1).unwrap();
    fs::write(&card1, yaoplm_test_support::emulator::ps2bbl_card()).unwrap();
    let asked = inspect(json!({}));
    assert_eq!(asked["loader"]["detected"]["library"], false);
    assert_eq!(asked["askProfile"], true);
    // A boot card named for what it does not hold is a warning.
    fs::write(
        device.join("MemoryCards/PS2/BOOT/BootCard.ini"),
        "[ChannelName]\n1=FMCB 1.966\n2=PS2BBL\n",
    )
    .unwrap();
    let named = inspect(json!({}));
    assert!(
        codes(&named).contains(&"emulatorChannelName".to_owned()),
        "{named}"
    );
    fs::write(&card1, with_mmce).unwrap();
    fs::write(
        device.join("MemoryCards/PS2/BOOT/BootCard.ini"),
        yaoplm_test_support::emulator::PS2_BOOT_INI,
    )
    .unwrap();

    // Read as a stick, the doctor's rules change; the protection does not.
    let key = info["loader"]["key"].as_str().unwrap().to_owned();
    let as_stick = inspect(json!({"modeOverrides": [{"key": key, "mode": "ps2"}]}));
    assert_eq!(as_stick["mode"]["mode"], "ps2");
    assert_eq!(as_stick["mode"]["detected"], "emulator");
    assert!(as_stick["emulator"].is_object());
    assert_eq!(as_stick["askProfile"], false);
    assert_eq!(
        inspect(json!({"mode": "emulator"}))["mode"]["source"],
        "explicit"
    );

    // OPL's folders go next to the emulator's files.
    let plan = call(&api, json!({"type": "PlanInit", "root": s.device})).unwrap();
    assert_eq!(plan["executable"], true, "{plan}");
    assert!(execute(&api, &plan, false).error.is_none());
    assert!(device.join("CD").is_dir() && device.join("DVD").is_dir());
    for (path, bytes) in yaoplm_test_support::emulator::files() {
        assert_eq!(fs::read(device.join(&path)).unwrap(), bytes, "{path}");
    }
    // On main a library folder could be made inside them.
    for folder in ["MemoryCards", ".sd2psx", "memorycards"] {
        let plan = call(
            &api,
            json!({"type": "PlanInit", "root": s.device, "folder": folder}),
        )
        .unwrap();
        assert_eq!(plan["executable"], false, "{folder}: {plan}");
        assert_eq!(plan["notices"][0]["code"], "emulatorServiceFile", "{plan}");
        let refused = call(&api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap_err();
        assert_eq!(refused.code, ErrorCode::BadRequest, "{folder}");
    }
    assert!(!device.join("MemoryCards").join("CFG").exists());
    assert!(!device.join(".sd2psx").join("CFG").exists());

    // A folder inside the SD is the SD too.
    let inner = device.join("CD");
    let info = call(
        &api,
        json!({"type": "InspectDevice", "root": inner.to_string_lossy()}),
    )
    .unwrap();
    assert_eq!(info["mode"]["mode"], "emulator");
    assert_eq!(info["emulator"]["folder"], "CD");
    assert_eq!(
        info["emulator"]["variants"][0]["boot"][0]["name"],
        "FMCB 1.966"
    );
}

#[test]
fn a_folder_on_the_computer_is_read_over_smb() {
    // ADR-0047: the place gives the mode. A temp folder is never removable.
    let s = setup();
    let api = Api::new();
    let inspect = |extra: Value| {
        let mut request = json!({"type": "InspectDevice", "root": s.device});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        call(&api, request).unwrap()
    };
    let codes = |device: &Value| -> Vec<String> {
        device["advice"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["code"].as_str().unwrap().to_owned())
            .collect()
    };

    let device = inspect(json!({}));
    assert_eq!(device["mode"]["mode"], "smb");
    assert_eq!(device["mode"]["source"], "detected");
    assert_eq!(device["mode"]["detected"], "smb");
    assert_eq!(
        device["mode"]["placeText"],
        "a folder on the computer’s disk"
    );
    // No system disk error, no file system rules: SMB's own.
    assert!(codes(&device).contains(&"smbRules".to_owned()), "{device}");
    assert!(
        device["advice"]
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["level"] != "error"),
        "{device}"
    );
    // Empty, and still asked: the console leaves no sign over SMB.
    assert_eq!(device["askProfile"], true);
    assert_eq!(device["loader"]["detected"]["library"], false);

    // Switched to the console mode for this device: the BDM rules, and an
    // empty device is not asked (it gets OPL installed first).
    let key = device["loader"]["key"].as_str().unwrap().to_owned();
    let device = inspect(json!({"modeOverrides": [{"key": key, "mode": "ps2"}]}));
    assert_eq!(
        (&device["mode"]["mode"], &device["mode"]["source"]),
        (&json!("ps2"), &json!("override"))
    );
    assert_eq!(device["mode"]["detected"], "smb");
    assert!(!codes(&device).contains(&"smbRules".to_owned()));
    assert_eq!(device["askProfile"], false);
    let device = inspect(json!({"mode": "ps2"}));
    assert_eq!(device["mode"]["source"], "explicit");

    // RiptOPL's settings in a folder read over SMB are a copy.
    let app = Path::new(&s.device).join("APPS").join("APP_RIPTOPL");
    fs::create_dir_all(&app).unwrap();
    fs::write(app.join("settings_riptopl.cfg"), "enable_usb=1\r\n").unwrap();
    let device = inspect(json!({}));
    assert_eq!(device["loader"]["profile"], "RiptOpl");
    assert_eq!(
        device["loader"]["detected"]["reason"]["key"],
        "reasons.profileByCopy"
    );
    assert_eq!(
        device["loader"]["detected"]["reasonText"],
        "APPS/APP_RIPTOPL/settings_riptopl.cfg, which only RiptOPL writes (a copy: over SMB the console starts OPL from a memory card or stick)"
    );
    assert!(codes(&device).contains(&"smbRulesSmb2".to_owned()));
    assert_eq!(device["askProfile"], false);
    let device = inspect(json!({"mode": "ps2"}));
    assert_eq!(
        device["loader"]["detected"]["reason"]["key"],
        "reasons.profileByFiles"
    );

    // A server root is a folder on the server.
    let root = fs::canonicalize(Path::new(&s.device)).unwrap();
    let server = Api::with_roots(vec![root]);
    let devices = call(&server, json!({"type": "ListDevices"})).unwrap();
    assert_eq!(
        devices["devices"][0]["mode"]["placeText"],
        "a folder on the server"
    );
}

#[test]
fn a_library_folder_inside_the_root() {
    let s = setup();
    let api = Api::new();
    let plan = call(
        &api,
        json!({"type": "PlanInit", "root": s.device, "folder": "PS2"}),
    )
    .unwrap();
    assert_eq!(
        plan["transactions"][0]["steps"].as_array().unwrap().len(),
        10
    );
    execute(&api, &plan, false);
    for name in ["CD", "DVD", "ART", "VMC"] {
        assert!(
            Path::new(&s.device).join("PS2").join(name).is_dir(),
            "{name}"
        );
    }
    let err = call(
        &api,
        json!({"type": "PlanInit", "root": s.device, "folder": "../out"}),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
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
            JobLogEntry::Art { .. } | JobLogEntry::Verify { .. } => {
                unreachable!("plans do not fetch art or verify")
            }
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
    assert_eq!(
        report.error_phrase.as_ref().map(|p| p.key.as_str()),
        Some("errors.alreadyExists")
    );
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
