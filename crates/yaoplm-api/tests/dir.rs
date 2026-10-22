//! `ListDir` through `dispatch`: the web UI's source picker never sees a
//! path outside the server's library roots (IMP-1, ADR-0009).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use yaoplm_api::dir::{DirListing, EntryKind};
use yaoplm_api::{Api, ErrorCode, Request, Response};

struct Tree {
    _dir: tempfile::TempDir,
    games: PathBuf,
    outside: PathBuf,
}

fn tree() -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let games = dir.path().join("games");
    let outside = dir.path().join("outside");
    fs::create_dir_all(games.join("more")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    for name in [
        "A.iso",
        "b.ZSO",
        "notes.txt",
        ".hidden",
        "Disc.cue",
        "disc.bin",
    ] {
        fs::write(games.join(name), b"x").unwrap();
    }
    fs::write(games.join("more/Z.iso"), b"xyz").unwrap();
    fs::write(outside.join("Secret.iso"), b"x").unwrap();
    Tree {
        games: fs::canonicalize(&games).unwrap(),
        outside: fs::canonicalize(&outside).unwrap(),
        _dir: dir,
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn list(api: &Api, path: Option<&Path>) -> Result<DirListing, yaoplm_api::ApiError> {
    match api.dispatch(Request::ListDir {
        path: path.map(text),
    })? {
        Response::Dir(listing) => Ok(*listing),
        other => panic!("unexpected answer {other:?}"),
    }
}

#[test]
fn server_lists_its_roots_and_nothing_above_them() {
    let t = tree();
    let api = Api::with_roots(vec![t.games.clone()]);

    let top = list(&api, None).unwrap();
    assert_eq!(top.path, None);
    assert_eq!(top.parent, None);
    assert_eq!(top.entries.len(), 1);
    assert_eq!(top.entries[0].name, "games");
    assert_eq!(top.entries[0].kind, EntryKind::Dir);

    let games = list(&api, Some(&t.games)).unwrap();
    // The root's parent is outside the allow-list: the UI goes back to the roots.
    assert_eq!(games.parent, None);
    let names: Vec<&str> = games.entries.iter().map(|e| e.name.as_str()).collect();
    // Folders first, then files sorted without regard to case; dotfiles hidden.
    assert_eq!(
        names,
        [
            "more",
            "A.iso",
            "b.ZSO",
            "disc.bin",
            "Disc.cue",
            "notes.txt"
        ]
    );
    let importable: Vec<bool> = games.entries.iter().map(|e| e.importable).collect();
    assert_eq!(importable, [false, true, true, true, true, false]);
    assert_eq!(games.entries[1].size, Some(1));
    assert!(!games.truncated);

    let more = list(&api, Some(&t.games.join("more"))).unwrap();
    assert_eq!(more.parent.as_deref(), games.path.as_deref());
    assert_eq!(more.entries[0].size, Some(3));
    assert!(Path::new(&more.entries[0].path).is_absolute());

    let err = list(&api, Some(&t.outside)).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = list(&api, Some(&t.games.join("../outside"))).unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    let err = list(&api, Some(&t.games.join("missing"))).unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    let err = list(&api, Some(&t.games.join("A.iso"))).unwrap_err();
    assert_eq!(err.code, ErrorCode::BadRequest);
}

#[test]
fn desktop_may_walk_up() {
    let t = tree();
    let api = Api::new();
    let games = list(&api, Some(&t.games)).unwrap();
    assert!(games.parent.is_some());
    let outside = list(&api, Some(&t.outside)).unwrap();
    assert_eq!(outside.entries[0].name, "Secret.iso");
}

#[test]
fn wire_format() {
    let t = tree();
    let api = Api::with_roots(vec![t.games.clone()]);
    let answer: Value =
        serde_json::to_value(api.dispatch_json(r#"{"type":"ListDir"}"#).unwrap()).unwrap();
    assert_eq!(answer["type"], "Dir");
    assert_eq!(answer["path"], Value::Null);
    assert_eq!(answer["entries"][0]["kind"], "dir");

    let request = json!({"type": "ListDir", "path": t.games}).to_string();
    let answer: Value = serde_json::to_value(api.dispatch_json(&request).unwrap()).unwrap();
    assert_eq!(answer["entries"][1]["importable"], true);
    assert_eq!(answer["truncated"], false);
}
