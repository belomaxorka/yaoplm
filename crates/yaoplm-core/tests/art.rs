//! Cover plans: write, replace with a backup, delete (ART-1, ART-3, ART-4).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use common::run;
use yaoplm_core::Media;
use yaoplm_core::art::{ArtItem, plan_delete_art, plan_set_art};
use yaoplm_core::exec::Outcome;
use yaoplm_core::ops::OpError;
use yaoplm_core::plan::{Level, Step};
use yaoplm_device::FsKind;
use yaoplm_formats::art::ArtKind;
use yaoplm_test_support::{Content, MemFs, VirtualSources};

fn item(startup: &str, file: &str) -> ArtItem {
    ArtItem {
        startup: startup.into(),
        title: format!("Game {startup}"),
        media: Media::Dvd,
        format: "ISO".into(),
        kind: ArtKind::Cov,
        file: PathBuf::from("cache").join(file),
        origin: "xlenore".into(),
    }
}

fn sources() -> VirtualSources {
    let src = VirtualSources::new();
    src.add_file("cache/a.png", Content::Bytes(b"PNG-A".to_vec()));
    src.add_file("cache/b.png", Content::Bytes(b"PNG-B".to_vec()));
    src
}

fn backups(fs: &MemFs) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = fs
        .files()
        .into_iter()
        .filter(|(p, _)| p.starts_with(".yaoplm/backup/"))
        .map(|(p, _)| {
            let bytes = fs.bytes(&p).unwrap();
            (p, bytes)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn covers_are_written_into_art_and_the_folder_is_created() {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("CD/SLUS_200.62.Retail.iso", b"iso");
    let src = sources();
    let plan = plan_set_art(
        &fs,
        &src,
        &[item("SLUS_200.62", "a.png"), item("SLUS_212.18", "b.png")],
        false,
    )
    .unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert_eq!(plan.title, "Write 2 covers");
    // Folder first, then one transaction per cover.
    assert_eq!(plan.transactions.len(), 3);
    assert!(matches!(
        plan.transactions[0].steps[0],
        Step::CreateDir { .. }
    ));
    assert_eq!(plan.transactions[1].commit, 1);
    assert_eq!(plan.write_bytes(), 10);
    assert!(!plan.deletes_user_data());
    let report = run(&plan, &fs, &src);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(report.outcomes, [Outcome::Done; 3]);
    assert_eq!(fs.bytes("ART/SLUS_200.62_COV.png").unwrap(), b"PNG-A");
    assert_eq!(fs.bytes("ART/SLUS_212.18_COV.png").unwrap(), b"PNG-B");
    assert!(backups(&fs).is_empty());
    assert!(
        fs.files()
            .iter()
            .all(|(p, _)| !p.starts_with(".yaoplm/journal/"))
    );
}

#[test]
fn existing_covers_are_skipped_or_moved_to_the_backup_folder() {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("art/slus_200.62_cov.png", b"old");
    let src = sources();

    let plan = plan_set_art(&fs, &src, &[item("SLUS_200.62", "a.png")], false).unwrap();
    assert!(!plan.is_executable());
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason.code, "coverExists");
    assert_eq!(plan.notices[0].code, "nothingToWrite");

    let plan = plan_set_art(&fs, &src, &[item("SLUS_200.62", "a.png")], true).unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert!(!plan.deletes_user_data(), "a move, not a deletion");
    let tx = &plan.transactions[1];
    assert!(
        matches!(&tx.steps[0], Step::Rename { from, .. } if from.as_str() == "art/slus_200.62_cov.png")
    );
    assert!(
        matches!(&tx.steps[1], Step::WriteFile { path, .. } if path.as_str() == "art/SLUS_200.62_COV.png")
    );
    assert!(tx.notices.iter().any(|n| n.code == "coverReplaced"));
    let report = run(&plan, &fs, &src);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(fs.bytes("art/SLUS_200.62_COV.png").unwrap(), b"PNG-A");
    let saved = backups(&fs);
    assert_eq!(saved.len(), 1);
    assert!(
        saved[0].0.ends_with("/art/slus_200.62_cov.png"),
        "{}",
        saved[0].0
    );
    assert_eq!(saved[0].1, b"old");
}

#[test]
fn a_failed_write_puts_the_old_cover_back() {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("ART/SLUS_200.62_COV.png", b"old");
    let src = sources();
    let plan = plan_set_art(&fs, &src, &[item("SLUS_200.62", "a.png")], true).unwrap();
    fs.fail_once("create", "ART/SLUS_200.62_COV.png.yaoplm-part");
    let report = run(&plan, &fs, &src);
    assert!(!report.succeeded());
    assert_eq!(report.outcomes[1], Outcome::RolledBack);
    assert_eq!(fs.bytes("ART/SLUS_200.62_COV.png").unwrap(), b"old");
    assert!(backups(&fs).is_empty());
    assert!(report.journal.is_none());
}

#[test]
fn unreadable_sources_and_duplicates_are_skipped() {
    let fs = MemFs::new(FsKind::ExFat);
    let src = sources();
    let plan = plan_set_art(
        &fs,
        &src,
        &[
            item("SLUS_200.62", "a.png"),
            item("slus_200.62", "b.png"),
            item("SLUS_212.18", "missing.png"),
        ],
        false,
    )
    .unwrap();
    assert_eq!(plan.transactions.len(), 2);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.skipped[0].reason.code, "coverUnreadable");
    assert_eq!(plan.skipped[0].reason.level, Level::Warning);

    // A changed source is refused at execution time.
    src.touch(&PathBuf::from("cache/a.png"));
    let report = run(&plan, &fs, &src);
    assert!(!report.succeeded());
    assert!(!fs.exists("ART/SLUS_200.62_COV.png"));
}

#[test]
fn deleting_a_cover_backs_it_up_and_needs_confirmation() {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("ART/SLUS_200.62_COV.png", b"png");
    fs.insert_file("ART/SLUS_200.62_COV.jpg", b"jpg");
    fs.insert_file("ART/SLUS_200.62_ICO.png", b"ico");
    let src = VirtualSources::new();
    let plan = plan_delete_art(&fs, "SLUS_200.62", ArtKind::Cov).unwrap();
    assert!(plan.deletes_user_data());
    assert_eq!(plan.delete_bytes(), 6);
    let report = run(&plan, &fs, &src);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(!fs.exists("ART/SLUS_200.62_COV.png"));
    assert!(!fs.exists("ART/SLUS_200.62_COV.jpg"));
    assert_eq!(fs.bytes("ART/SLUS_200.62_ICO.png").unwrap(), b"ico");
    let saved = backups(&fs);
    assert_eq!(saved.len(), 2);
    assert!(
        saved
            .iter()
            .any(|(p, b)| p.ends_with("/ART/SLUS_200.62_COV.png") && b == b"png")
    );

    assert!(matches!(
        plan_delete_art(&fs, "SLUS_200.62", ArtKind::Cov),
        Err(OpError::NotFound(_))
    ));
}
