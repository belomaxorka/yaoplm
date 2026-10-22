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

#[test]
fn rpc_reads_request_from_stdin() {
    let mut child = yaoplm()
        .arg("rpc")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"type":"Version"}"#)
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());

    let answer: serde_json::Value = serde_json::from_str(stdout(&output)).unwrap();
    assert_eq!(answer["type"], "Version");
    assert_eq!(answer["version"], env!("CARGO_PKG_VERSION"));
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
