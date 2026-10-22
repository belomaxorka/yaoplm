#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn yaoplm() -> Command {
    Command::new(env!("CARGO_BIN_EXE_yaoplm"))
}

fn stdout(output: &Output) -> &str {
    std::str::from_utf8(&output.stdout).unwrap().trim()
}

#[test]
fn prints_version() {
    let output = yaoplm().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        stdout(&output),
        format!("yaoplm {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn rpc_ping_from_argument() {
    let output = yaoplm()
        .args(["rpc", r#"{"type":"Ping"}"#])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(stdout(&output), r#"{"type":"Pong"}"#);
}

fn rpc_stdin(input: &[u8]) -> Output {
    let mut child = yaoplm()
        .arg("rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn rpc_reads_request_from_stdin() {
    let output = rpc_stdin(br#"{"type":"Version"}"#);
    assert!(output.status.success());

    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "Version");
    assert_eq!(answer["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn rpc_stdin_tolerates_utf8_bom() {
    let output = rpc_stdin(b"\xef\xbb\xbf{\"type\":\"Ping\"}\r\n");
    assert!(output.status.success());
    assert_eq!(stdout(&output), r#"{"type":"Pong"}"#);
}

#[test]
fn rpc_error_prints_json_and_fails() {
    let output = yaoplm()
        .args(["rpc", r#"{"type":"Nope"}"#])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));

    let error: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(error["code"], "BadRequest");
}

fn library() -> tempfile::TempDir {
    use yaoplm_test_support::Variant;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("CD")).unwrap();
    std::fs::create_dir_all(dir.path().join("DVD")).unwrap();
    let retail = Variant::Retail.build().to_vec();
    std::fs::write(dir.path().join("CD/SLUS_200.62.GTA III.iso"), retail).unwrap();
    std::fs::write(
        dir.path().join("DVD/No Boot.iso"),
        Variant::NoBoot2.build().to_vec(),
    )
    .unwrap();
    dir
}

#[test]
fn scan_prints_the_library() {
    let dir = library();
    let output = yaoplm().arg("scan").arg(dir.path()).output().unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("OPL 1.2.0 beta"), "{text}");
    assert!(text.contains("SLUS_200.62"), "{text}");
    assert!(text.contains("GTA III"), "{text}");
    assert!(text.contains("Not listed by OPL (1)"), "{text}");
    assert!(text.contains("No Boot.iso"), "{text}");
}

#[test]
fn scan_json_matches_the_api() {
    let dir = library();
    let output = yaoplm()
        .args(["scan", "--json", "--profile", "opl-1.1"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "Library");
    assert_eq!(answer["profile"], "Opl110");
    assert_eq!(answer["games"][0]["nameFormat"], "old");
}

#[test]
fn info_describes_an_image() {
    let dir = library();
    let output = yaoplm()
        .arg("info")
        .arg(dir.path().join("CD/SLUS_200.62.GTA III.iso"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let text = stdout(&output);
    assert!(text.contains("Game ID:        SLUS_200.62"), "{text}");
    assert!(text.contains("old style"), "{text}");

    let hidden = yaoplm()
        .args(["info", "--json"])
        .arg(dir.path().join("DVD/No Boot.iso"))
        .output()
        .unwrap();
    let answer: serde_json::Value = serde_json::from_str(stdout(&hidden)).unwrap();
    assert_eq!(answer["startup"], serde_json::Value::Null);
}

#[test]
fn scan_of_a_missing_folder_fails() {
    let dir = tempfile::tempdir().unwrap();
    let output = yaoplm()
        .arg("scan")
        .arg(dir.path().join("nope"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("yaoplm:"));
}

struct Stick {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    games: std::path::PathBuf,
}

fn stick() -> Stick {
    use yaoplm_test_support::Variant;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("stick");
    let games = dir.path().join("games");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&games).unwrap();
    std::fs::write(games.join("Retail.iso"), Variant::Retail.build().to_vec()).unwrap();
    std::fs::write(games.join("Dvd Nine.iso"), Variant::Dvd9.build().to_vec()).unwrap();
    Stick {
        root,
        games,
        _dir: dir,
    }
}

fn run(args: &[&std::ffi::OsStr]) -> Output {
    yaoplm().args(args).stdin(Stdio::null()).output().unwrap()
}

#[test]
fn add_prints_the_plan_and_needs_yes() {
    let s = stick();
    let dry = run(&["add".as_ref(), s.root.as_os_str(), s.games.as_os_str()]);
    assert!(dry.status.success());
    let text = stdout(&dry);
    assert!(text.contains("Plan: Import 2 games"), "{text}");
    assert!(text.contains("+ write CD/SLUS_200.62.Retail.iso"), "{text}");
    assert!(
        text.contains("+ write DVD/SLUS_212.18.Dvd Nine.iso"),
        "{text}"
    );
    assert!(String::from_utf8_lossy(&dry.stderr).contains("--yes"));
    assert!(
        std::fs::read_dir(&s.root).unwrap().next().is_none(),
        "nothing written"
    );

    let done = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(
        done.status.success(),
        "{}",
        String::from_utf8_lossy(&done.stderr)
    );
    assert!(stdout(&done).contains("done  Copy SLUS_200.62.Retail.iso to CD"));
    assert!(s.root.join("CD/SLUS_200.62.Retail.iso").is_file());
    assert!(s.root.join("APPS").is_dir());

    // The same games again: skipped, nothing to apply.
    let again = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.as_os_str(),
        "--yes".as_ref(),
    ]);
    assert_eq!(again.status.code(), Some(1));
    assert!(stdout(&again).contains("already exists"));
}

#[test]
fn rename_move_and_remove_by_id() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--yes".as_ref(),
    ]);
    assert!(add.status.success());

    let rename = run(&[
        "rename".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "New Name".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rename.status.success(),
        "{}",
        String::from_utf8_lossy(&rename.stderr)
    );
    assert!(s.root.join("CD/SLUS_200.62.New Name.iso").is_file());

    let mv = run(&[
        "mv".as_ref(),
        s.root.as_os_str(),
        "New Name".as_ref(),
        "--to".as_ref(),
        "dvd".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        mv.status.success(),
        "{}",
        String::from_utf8_lossy(&mv.stderr)
    );
    assert!(s.root.join("DVD/SLUS_200.62.New Name.iso").is_file());

    // Deleting shows DELETE in the plan and asks for --yes.
    let dry = run(&["rm".as_ref(), s.root.as_os_str(), "SLUS_200.62".as_ref()]);
    assert!(stdout(&dry).contains("- DELETE DVD/SLUS_200.62.New Name.iso"));
    assert!(s.root.join("DVD/SLUS_200.62.New Name.iso").is_file());
    let rm = run(&[
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_200.62".as_ref(),
        "--yes".as_ref(),
    ]);
    assert!(rm.status.success());
    assert!(!s.root.join("DVD/SLUS_200.62.New Name.iso").exists());

    let missing = run(&[
        "rm".as_ref(),
        s.root.as_os_str(),
        "SLUS_999.99".as_ref(),
        "--yes".as_ref(),
    ]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("no game matches"));
}

#[test]
fn ul_games_on_request_and_recover_without_journals() {
    let s = stick();
    let add = run(&[
        "add".as_ref(),
        s.root.as_os_str(),
        s.games.join("Retail.iso").as_os_str(),
        "--ul".as_ref(),
        "--title".as_ref(),
        "Retail Ul".as_ref(),
        "--yes".as_ref(),
        "--json".as_ref(),
    ]);
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    assert!(s.root.join("ul.cfg").is_file());
    let rename = run(&[
        "rename".as_ref(),
        s.root.as_os_str(),
        "ul:0".as_ref(),
        "Short".as_ref(),
        "-y".as_ref(),
    ]);
    assert!(
        rename.status.success(),
        "{}",
        String::from_utf8_lossy(&rename.stderr)
    );
    let scan = run(&["scan".as_ref(), s.root.as_os_str()]);
    assert!(
        stdout(&scan).contains("Short (1 part)"),
        "{}",
        stdout(&scan)
    );

    let recover = run(&["recover".as_ref(), s.root.as_os_str()]);
    assert!(recover.status.success());
    assert!(stdout(&recover).contains("no unfinished operation"));
}

#[test]
fn devices_and_device_doctor() {
    let s = stick();
    let all = run(&["devices".as_ref(), "--all".as_ref()]);
    assert!(all.status.success());
    assert!(!stdout(&all).is_empty());
    let device = run(&["device".as_ref(), s.root.as_os_str()]);
    assert!(device.status.success());
    let json = run(&["device".as_ref(), s.root.as_os_str(), "--json".as_ref()]);
    let answer: serde_json::Value = serde_json::from_str(stdout(&json)).unwrap();
    assert_eq!(answer["type"], "Device");
}

#[test]
fn rpc_waits_for_jobs() {
    let s = stick();
    let request = serde_json::json!({"type": "ScanLibrary", "root": s.root}).to_string();
    let output = yaoplm().args(["rpc", &request]).output().unwrap();
    assert!(output.status.success());
    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "JobState");
    assert_eq!(answer["status"], "finished");
    assert_eq!(answer["result"]["type"], "Library");
}
