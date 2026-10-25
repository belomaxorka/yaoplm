//! Interrupted writes: rollback, resume and recovery from the journal
//! (IMP-5, IMP-9, SPEC §6).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{GIB, assert_same_as, big_iso, files, run, run_with, src};
use yaoplm_core::exec::Outcome;
use yaoplm_core::import::{ImportOptions, Layout, plan_import};
use yaoplm_core::plan::{PlanKind, Step};
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_device::{CancelToken, FsKind};
use yaoplm_formats::ul_cfg::UlCfg;
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

fn ul_names(fs: &MemFs) -> Vec<String> {
    files(fs)
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.starts_with("ul."))
        .collect()
}

fn journals(fs: &MemFs) -> usize {
    fs.files()
        .iter()
        .filter(|(p, _)| p.starts_with(".yaoplm/journal/") && p.ends_with(".json"))
        .count()
}

/// A 4.4 GB game being split on FAT32 when the stick is pulled out in
/// the middle of chunk 2.
fn interrupted_split() -> (MemFs, VirtualSources, yaoplm_test_support::VirtualIso) {
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    let iso = big_iso("SLUS_212.18", 4_400_000_000);
    sources.add_iso(src("Huge.iso"), iso.clone());
    let plan = plan_import(&fs, &sources, &[src("Huge.iso")], &ImportOptions::default()).unwrap();
    fs.unplug_after(2 * GIB + 300_000_000);
    let report = run(&plan, &fs, &sources);
    assert!(!report.succeeded());
    assert_eq!(
        report.outcomes[0],
        Outcome::Done,
        "the folders were created"
    );
    assert_eq!(report.outcomes[1], Outcome::Interrupted);
    assert!(report.journal.is_some());
    fs.replug();
    (fs, sources, iso)
}

#[test]
fn a_damaged_journal_is_reported_not_a_panic() {
    let (fs, _sources, _) = interrupted_split();
    let (path, _) = fs
        .files()
        .into_iter()
        .find(|(p, _)| p.starts_with(".yaoplm/journal/") && p.ends_with(".json"))
        .unwrap();
    let text = String::from_utf8(fs.bytes(&path).unwrap()).unwrap();
    // The ul.cfg write keeps its content as hex; a byte out of place on
    // the stick splits a character across two hex digits.
    let key = "\"content\": \"";
    let at = text.find(key).expect("the ul.cfg write is journalled") + key.len();
    fs.insert_file(
        &path,
        format!("{}aéb{}", &text[..at], &text[at..]).as_bytes(),
    );
    let plan = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    assert!(
        plan.notices.iter().any(|n| n.code == "journalUnreadable"),
        "{:?}",
        plan.notices
    );
    assert!(!plan.is_executable());
}

#[test]
fn pulled_stick_is_rolled_back_from_the_journal() {
    let (fs, sources, _) = interrupted_split();
    // Two finished chunks and a partial one; OPL sees nothing yet.
    let names = ul_names(&fs);
    assert_eq!(names.len(), 3, "{names:?}");
    assert!(names[2].ends_with(".02.yaoplm-part"));
    assert!(!fs.exists("ul.cfg"));
    assert_eq!(journals(&fs), 1);

    // Nothing else may run until the device is recovered.
    let blocked =
        plan_import(&fs, &sources, &[src("Huge.iso")], &ImportOptions::default()).unwrap();
    assert!(!blocked.is_executable());

    let plan = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    assert_eq!(plan.kind, PlanKind::Recovery);
    assert!(plan.transactions[0].title.starts_with("Undo"));
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(ul_names(&fs).is_empty(), "{:?}", fs.files());
    assert_eq!(journals(&fs), 0);
    // The folder set-up was its own finished transaction and stays.
    assert!(fs.exists("DVD") && fs.exists("CFG"));
}

#[test]
fn pulled_stick_is_resumed_from_the_journal() {
    let (fs, sources, iso) = interrupted_split();
    let plan = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    assert!(plan.transactions[0].title.starts_with("Finish"));
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    let names = ul_names(&fs);
    assert_eq!(names.len(), 6, "{names:?}");
    for (part, name) in names.iter().filter(|n| *n != "ul.cfg").enumerate() {
        assert_same_as(&fs, name, &iso, part as u64 * GIB);
    }
    let table = UlCfg::parse(&fs.bytes("ul.cfg").unwrap());
    assert_eq!(table.records.len(), 1);
    assert_eq!(journals(&fs), 0);
}

#[test]
fn an_interrupted_recovery_can_run_again() {
    let (fs, sources, _) = interrupted_split();
    let plan = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    fs.unplug_after(GIB / 2);
    assert!(!run(&plan, &fs, &sources).succeeded());
    fs.replug();
    assert_eq!(journals(&fs), 2, "the import's and the recovery's");

    // The recovery's own journal covers the import's: planned once.
    let again = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    let report = run(&again, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(ul_names(&fs).len(), 6);
    assert_eq!(journals(&fs), 0);
    assert!(
        plan_recovery(&fs, RecoveryAction::Rollback)
            .unwrap()
            .notices
            .iter()
            .any(|n| n.code == "nothingToRecover")
    );
}

#[test]
fn cancel_rolls_back_the_current_game_only() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_iso(src("A.iso"), Variant::Retail.build());
    sources.add_iso(src("Huge.iso"), big_iso("SLUS_212.18", 2 * GIB));
    sources.add_iso(src("C.iso"), Variant::LongId.build());
    let inputs = [src("A.iso"), src("Huge.iso"), src("C.iso")];
    let plan = plan_import(&fs, &sources, &inputs, &ImportOptions::default()).unwrap();
    let cancel = CancelToken::new();
    let trigger = cancel.clone();
    let report = run_with(&plan, &fs, &sources, &cancel, &mut |p| {
        if p.title.contains("Huge") && p.done > GIB {
            trigger.cancel();
        }
    });
    assert!(report.cancelled);
    assert!(report.error.is_none());
    assert_eq!(
        report.outcomes,
        [
            Outcome::Done,
            Outcome::Done,
            Outcome::RolledBack,
            Outcome::NotStarted
        ]
    );
    assert!(report.journal.is_none());
    let names: Vec<String> = files(&fs).into_iter().map(|(p, _)| p).collect();
    assert_eq!(names, ["CD/SLUS_200.62.A.iso"]);
    assert_eq!(journals(&fs), 0);
}

#[test]
fn a_failed_ul_cfg_write_undoes_the_chunks() {
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    sources.add_iso(src("A.iso"), Variant::Retail.build());
    let forced = ImportOptions {
        layout: Layout::Ul,
        ..ImportOptions::default()
    };
    let plan = plan_import(&fs, &sources, &[src("A.iso")], &forced).unwrap();
    fs.fail_once("rename", "ul.cfg.yaoplm-tmp");
    let report = run(&plan, &fs, &sources);
    assert!(report.error.is_some());
    assert_eq!(report.outcomes[1], Outcome::RolledBack);
    assert!(ul_names(&fs).is_empty(), "{:?}", fs.files());
    assert_eq!(journals(&fs), 0);
}

#[test]
fn a_failure_after_the_commit_is_finished_later() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file("CD/games.bin", &[0u8; 188]);
    let sources = VirtualSources::new();
    sources.add_iso(src("A.iso"), Variant::Retail.build());
    let plan = plan_import(&fs, &sources, &[src("A.iso")], &ImportOptions::default()).unwrap();
    fs.fail_once("remove", "games.bin");
    let report = run(&plan, &fs, &sources);
    assert_eq!(report.outcomes.last(), Some(&Outcome::Interrupted));
    // The game is in place; only the cache removal is left.
    assert!(fs.exists("CD/SLUS_200.62.A.iso"));
    assert!(fs.exists("CD/games.bin"));

    // Even "rollback" finishes a committed transaction.
    let plan = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    let finish = &plan.transactions[0];
    assert!(finish.notices.iter().any(|n| n.code == "alreadyCommitted"));
    assert!(matches!(finish.steps[0], Step::Remove { .. }));
    assert!(run(&plan, &fs, &sources).succeeded());
    assert!(!fs.exists("CD/games.bin"));
    assert!(fs.exists("CD/SLUS_200.62.A.iso"));
    assert_eq!(journals(&fs), 0);
}
