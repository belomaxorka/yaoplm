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
