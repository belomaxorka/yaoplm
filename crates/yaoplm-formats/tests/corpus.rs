//! Real-world files from the reference corpus (docs/research/REFERENCES.md
//! §5). Needs `YAOPLM_TESTDATA` (e.g. `D:\yaoplm-testdata`); run with
//! `cargo test -p yaoplm-formats --test corpus -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use yaoplm_formats::cfg::{self, CfgDocument, Dialect};
use yaoplm_formats::cht::ChtFile;

fn refs() -> PathBuf {
    let root = std::env::var_os("YAOPLM_TESTDATA").expect("set YAOPLM_TESTDATA");
    PathBuf::from(root).join("refs")
}

fn files_with_extension(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != ".git") {
                files_with_extension(&path, ext, out);
            }
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
        {
            out.push(path);
        }
    }
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA"]
fn community_cfg_files_round_trip() {
    let mut files = Vec::new();
    for repo in ["Tom-Bruise__PS2-OPL-CFG-Database", "VTSTech__PS2-OPL-CFG"] {
        files_with_extension(&refs().join(repo), "cfg", &mut files);
    }
    assert!(
        files.len() > 10_000,
        "corpus missing? found {}",
        files.len()
    );

    let mut keys = 0;
    let mut with_compat = 0;
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(
            cfg::detect_dialect(&bytes),
            Dialect::Opl,
            "{}",
            path.display()
        );
        let doc = CfgDocument::parse(&bytes);
        assert_eq!(doc.to_bytes(), bytes, "{}", path.display());
        let entries = doc.entries();
        keys += entries.len();
        with_compat += usize::from(doc.get("$Compatibility").is_some());

        // Editing one key leaves every other key and its order alone.
        let mut edited = doc.clone();
        edited.set("$Compatibility", "3").unwrap();
        let others = |d: &CfgDocument| {
            d.entries()
                .into_iter()
                .filter(|e| e.key != "$Compatibility")
                .collect::<Vec<_>>()
        };
        assert_eq!(others(&edited), others(&doc), "{}", path.display());
    }
    println!(
        "{} CFG files, {keys} keys, {with_compat} with $Compatibility",
        files.len()
    );
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA"]
fn community_cheat_files_parse() {
    let mut files = Vec::new();
    files_with_extension(
        &refs().join("PS2-Widescreen__OPL-Widescreen-Cheats"),
        "cht",
        &mut files,
    );
    assert!(files.len() > 100, "corpus missing? found {}", files.len());

    let (mut cheats, mut warned, mut with_master) = (0, 0, 0);
    for path in &files {
        let file = ChtFile::parse(&std::fs::read(path).unwrap());
        cheats += file.cheats.len();
        warned += usize::from(!file.warnings.is_empty());
        with_master += usize::from(file.cheats.iter().any(|c| c.is_master_code()));
    }
    println!(
        "{} CHT files, {cheats} cheats, {with_master} with a master code, {warned} with warnings",
        files.len()
    );
}
