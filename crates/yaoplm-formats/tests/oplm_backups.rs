//! Picture-set names against real OPL Manager art backups (ART-5,
//! research/13 §3): every picture of every game in the tables of contents
//! of the archive.org backups must be read as the type its name says, under
//! the ID of its folder. Needs `YAOPLM_TESTDATA` with `oplm-art/*.tsv`,
//! written by `scripts/oplm-backup-toc.py` (only the zip's central
//! directory is fetched, never the pictures); run with
//! `cargo test -p yaoplm-formats --test oplm_backups -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use yaoplm_formats::art::{ArtKind, pack_picture};

/// What a backup name says, read independently of `pack_picture`.
fn expected(file: &str) -> Option<ArtKind> {
    let stem = file.rsplit_once('.')?.0.to_ascii_uppercase();
    let suffix = |s: &str| stem.ends_with(&format!("_{s}"));
    Some(if suffix("SCR_00") {
        ArtKind::Scr
    } else if suffix("SCR_01") {
        ArtKind::Scr2
    } else if suffix("BG_00") {
        ArtKind::Bg
    } else if suffix("COV2") {
        ArtKind::Cov2
    } else if suffix("COV") {
        ArtKind::Cov
    } else if suffix("ICO") {
        ArtKind::Ico
    } else if suffix("LAB") {
        ArtKind::Lab
    } else if suffix("LGO") {
        ArtKind::Lgo
    } else {
        // Further screenshots and backgrounds have no place in OPL.
        return None;
    })
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with oplm-art tables of contents"]
fn every_name_in_the_real_backups_is_read() {
    let root = std::env::var_os("YAOPLM_TESTDATA").expect("set YAOPLM_TESTDATA");
    let dir = PathBuf::from(root).join("oplm-art");
    let mut tables: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "tsv"))
        .collect();
    tables.sort();
    assert!(
        !tables.is_empty(),
        "no tables of contents in {}",
        dir.display()
    );
    for table in tables {
        let text = std::fs::read_to_string(&table).unwrap();
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        let mut wrong = Vec::new();
        let (mut pictures, mut left_out) = (0, 0);
        for line in text.lines() {
            let name = line.split('\t').next().unwrap();
            let parts: Vec<&str> = name.split('/').collect();
            let [console, folder, file] = parts[..] else {
                continue;
            };
            if file.is_empty() || !matches!(console, "PS1" | "PS2") {
                continue;
            }
            pictures += 1;
            let read = pack_picture(file);
            match (expected(file), &read) {
                (None, None) => left_out += 1,
                (Some(kind), Some(p)) if p.kind == kind && p.key.eq_ignore_ascii_case(folder) => {
                    *kinds
                        .entry(format!("{console} {:?} .{}", kind, p.extension))
                        .or_default() += 1;
                }
                _ => wrong.push(format!("{name} -> {read:?}")),
            }
        }
        println!(
            "{}: {pictures} pictures, {left_out} left out (SCR_02+, BG_01+)",
            table.file_name().unwrap().to_string_lossy()
        );
        for (kind, count) in &kinds {
            println!("  {kind}: {count}");
        }
        assert!(pictures > 10_000, "{}: too few pictures", table.display());
        assert!(
            wrong.is_empty(),
            "{} misread: {:#?}",
            wrong.len(),
            &wrong[..wrong.len().min(20)]
        );
    }
}
