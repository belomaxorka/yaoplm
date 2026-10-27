//! A game's settings and memory cards through `dispatch` (CFG-2, VMC-1),
//! on a temp folder as the device.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobResult, JobStatus};
use yaoplm_api::{Api, ErrorCode, JobId};
use yaoplm_formats::vmc;
use yaoplm_test_support::Variant;

struct Device {
    _dir: tempfile::TempDir,
    root: String,
    path: std::path::PathBuf,
}

fn device() -> Device {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stick");
    fs::create_dir_all(path.join("CD")).unwrap();
    fs::create_dir_all(path.join("CFG")).unwrap();
    fs::write(
        path.join("CD").join("SLUS_200.62.Retail.iso"),
        Variant::Retail.build().to_vec(),
    )
    .unwrap();
    fs::write(
        path.join("CFG").join("SLUS_200.62.cfg"),
        "Title=Retail\r\n$Compatibility=1\r\n",
    )
    .unwrap();
    Device {
        root: path.to_string_lossy().into_owned(),
        path,
        _dir: dir,
    }
}

fn call(api: &Api, request: Value) -> Result<Value, yaoplm_api::ApiError> {
    api.dispatch_json(&request.to_string())
        .map(|r| serde_json::to_value(r).unwrap())
}

fn execute(api: &Api, plan: &Value) -> yaoplm_api::jobs::ExecutionReport {
    let started = call(api, json!({"type": "ExecutePlan", "plan": plan["id"]})).unwrap();
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::Execution(report)) => report,
        other => panic!("unexpected result {other:?}"),
    }
}

fn game() -> Value {
    json!({"kind": "image", "path": "CD/SLUS_200.62.Retail.iso"})
}

#[test]
fn settings_are_read_planned_and_written() {
    let device = device();
    let api = Api::new();
    let cfg = call(
        &api,
        json!({"type": "ReadCfg", "root": device.root, "game": game()}),
    )
    .unwrap();
    assert_eq!(cfg["type"], "Cfg");
    assert_eq!(cfg["startup"], "SLUS_200.62");
    assert_eq!(cfg["exists"], true);
    assert_eq!(cfg["editable"], true);
    assert_eq!(cfg["settings"]["compatibility"], 1);
    assert_eq!(cfg["settings"]["vmc"], json!(["", ""]));
    assert_eq!(cfg["cardNames"], json!(["SLUS_200.62_0", "SLUS_200.62_1"]));
    assert_eq!(
        cfg["entries"][0],
        json!({"key": "Title", "value": "Retail"})
    );

    // Mode 2 instead of 1, and a new card in slot 1.
    let mut settings = cfg["settings"].clone();
    settings["compatibility"] = json!(2);
    settings["vmc"] = json!(["SLUS_200.62_0", ""]);
    let plan = call(
        &api,
        json!({
            "type": "WriteCfg",
            "root": device.root,
            "game": game(),
            "settings": settings,
            "cards": [{"name": "SLUS_200.62_0", "sizeMb": 8}],
        }),
    )
    .unwrap();
    assert_eq!(plan["kind"], "settings");
    assert_eq!(plan["executable"], true);
    assert_eq!(plan["deletesUserData"], false);
    let tx = &plan["transactions"][0];
    assert_eq!(tx["game"]["startup"], "SLUS_200.62");
    assert_eq!(
        tx["cfgChanges"],
        json!([
            {"key": "$Compatibility", "before": "1", "after": "2"},
            {"key": "$ConfigSource", "before": null, "after": "1"},
            {"key": "$VMC_0", "before": null, "after": "SLUS_200.62_0"},
        ])
    );
    let ops: Vec<&str> = tx["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["op"].as_str().unwrap())
        .collect();
    assert_eq!(ops, ["createDir", "writeVmc", "copyFile", "putFile"]);
    assert_eq!(tx["steps"][1]["bytes"], 8 << 20);

    let report = execute(&api, &plan);
    assert!(report.error.is_none(), "{report:?}");
    assert!(report.notices.is_empty(), "{report:?}");
    assert_eq!(
        fs::read(device.path.join("CFG").join("SLUS_200.62.cfg")).unwrap(),
        b"Title=Retail\r\n$Compatibility=2\r\n$ConfigSource=1\r\n$VMC_0=SLUS_200.62_0\r\n"
    );
    let card = fs::read(device.path.join("VMC").join("SLUS_200.62_0.bin")).unwrap();
    assert_eq!(vmc::opl_check(&card, card.len() as u64), Ok(8));

    let listed = call(&api, json!({"type": "ListVmc", "root": device.root})).unwrap();
    assert_eq!(listed["type"], "Vmcs");
    let cards = listed["cards"].as_array().unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["name"], "SLUS_200.62_0");
    assert_eq!(cards[0]["sizeMb"], 8);
    assert_eq!(cards[0]["saves"], 0);
    assert_eq!(
        cards[0]["usedBy"],
        json!([{"startup": "SLUS_200.62", "slot": 0}])
    );
}

#[test]
fn raw_keys_and_refusals() {
    let device = device();
    let api = Api::new();
    let plan = call(
        &api,
        json!({
            "type": "WriteCfg",
            "root": device.root,
            "game": game(),
            "set": [{"key": "Genre", "value": "Action"}],
            "unset": ["Title"],
        }),
    )
    .unwrap();
    execute(&api, &plan);
    assert_eq!(
        fs::read(device.path.join("CFG").join("SLUS_200.62.cfg")).unwrap(),
        b"$Compatibility=1\r\nGenre=Action\r\n"
    );

    let err = call(
        &api,
        json!({
            "type": "WriteCfg",
            "root": device.root,
            "game": game(),
            "set": [{"key": "$Compatibility", "value": "128"}],
        }),
    )
    .unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
    assert_eq!(err.phrase.as_ref().unwrap().key, "errors.settingsInvalid");
    // RiptOPL has a mode 8.
    let plan = call(
        &api,
        json!({
            "type": "WriteCfg",
            "root": device.root,
            "game": game(),
            "profile": "RiptOpl",
            "set": [{"key": "$Compatibility", "value": "128"}],
        }),
    );
    assert!(plan.is_ok(), "{plan:?}");
}

#[test]
fn blank_cards_on_their_own_and_replacing_one() {
    let device = device();
    let api = Api::new();
    fs::create_dir_all(device.path.join("VMC")).unwrap();
    // A zero-filled "card" another manager made: OPL refuses it.
    fs::write(
        device.path.join("VMC").join("generic_0.bin"),
        vec![0u8; 8 << 20],
    )
    .unwrap();
    let listed = call(&api, json!({"type": "ListVmc", "root": device.root})).unwrap();
    assert_eq!(listed["cards"][0]["sizeMb"], Value::Null);
    assert_eq!(
        listed["cards"][0]["problemPhrase"]["key"],
        "reasons.vmcBadMagic"
    );

    let request = |replace: bool| {
        json!({
            "type": "CreateVmc",
            "root": device.root,
            "cards": [{"name": "generic_0", "sizeMb": 16, "replace": replace}],
        })
    };
    let err = call(&api, request(false)).unwrap_err();
    assert_eq!(err.phrase.as_ref().unwrap().key, "errors.vmcExists");
    let plan = call(&api, request(true)).unwrap();
    assert_eq!(plan["kind"], "vmc");
    assert_eq!(plan["transactions"][0]["notices"][0]["code"], "vmcReplaced");
    execute(&api, &plan);
    let card = fs::read(device.path.join("VMC").join("generic_0.bin")).unwrap();
    assert_eq!(vmc::opl_check(&card, card.len() as u64), Ok(16));
    // The old file is kept in the backup folder.
    let backup = fs::read_dir(device.path.join(".yaoplm").join("backup"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let kept = fs::read(backup.join("VMC").join("generic_0.bin")).unwrap();
    assert_eq!(kept.len(), 8 << 20);
}

#[test]
fn wopl_files_are_shown_but_not_rewritten() {
    let device = device();
    fs::write(
        device.path.join("CFG").join("SLUS_200.62.cfg"),
        "compat :\n{\n  mode = 3;\n};\n",
    )
    .unwrap();
    let api = Api::new();
    let cfg = call(
        &api,
        json!({"type": "ReadCfg", "root": device.root, "game": game()}),
    )
    .unwrap();
    assert_eq!(cfg["editable"], false);
    let err = call(
        &api,
        json!({"type": "WriteCfg", "root": device.root, "game": game(), "set": [{"key": "a", "value": "b"}]}),
    )
    .unwrap_err();
    assert_eq!(err.phrase.as_ref().unwrap().key, "errors.cfgNotOpl");
}

// The bundled game info written by a plan (META-3, ADR-0050).
#[test]
fn game_info_is_planned_from_the_bundled_table() {
    let device = device();
    let api = Api::new();
    let plan = call(
        &api,
        json!({"type": "PlanGameInfo", "root": device.root, "game": game()}),
    )
    .unwrap();
    assert_eq!(plan["kind"], "settings");
    let tx = &plan["transactions"][0];
    assert_eq!(tx["titlePhrase"]["key"], "titles.gameInfo");
    let keys: Vec<&str> = tx["cfgChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "Genre",
            "Release",
            "Developer",
            "Publisher",
            "Players",
            "PlayersText"
        ]
    );
    assert_eq!(
        tx["cfgChanges"][2],
        json!({"key": "Developer", "before": null, "after": "DMA Design"})
    );
    let report = execute(&api, &plan);
    assert!(report.error.is_none(), "{report:?}");
    assert_eq!(
        fs::read(device.path.join("CFG").join("SLUS_200.62.cfg")).unwrap(),
        b"Title=Retail\r\n$Compatibility=1\r\nGenre=Action\r\nRelease=10-23-2001\r\nDeveloper=DMA Design\r\nPublisher=Rockstar Games\r\nPlayers=players/1\r\nPlayersText=1\r\n"
    );

    // `ReadCfg` shows the table next to the file.
    let cfg = call(
        &api,
        json!({"type": "ReadCfg", "root": device.root, "game": game()}),
    )
    .unwrap();
    assert_eq!(cfg["info"]["developer"]["value"], "DMA Design");
    assert_eq!(cfg["info"]["developer"]["source"], "libretro");
    assert_eq!(cfg["info"]["genres"]["value"], json!(["action"]));

    // Nothing left to write; then one chosen field plus a description.
    let again = call(
        &api,
        json!({"type": "PlanGameInfo", "root": device.root, "game": game()}),
    )
    .unwrap();
    assert!(again["transactions"].as_array().unwrap().is_empty());
    assert_eq!(again["notices"][0]["code"], "gameInfoUnchanged");
    let partial = call(
        &api,
        json!({
            "type": "PlanGameInfo", "root": device.root, "game": game(),
            "fields": ["genre"],
            "description": {"text": "First sentence. Second sentence.", "source": "https://en.wikipedia.org/wiki/Grand_Theft_Auto_III"}
        }),
    )
    .unwrap();
    let keys: Vec<&str> = partial["transactions"][0]["cfgChanges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["Description", "DescriptionSource"]);
}
