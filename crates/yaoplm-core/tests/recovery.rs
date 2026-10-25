//! Interrupted writes: rollback, resume and recovery from the journal
//! (IMP-5, IMP-9, SPEC §6).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{GIB, assert_same_as, big_iso, files, run, run_with, src};
use yaoplm_core::exec::Outcome;
use yaoplm_core::import::{ImportOptions, Layout, plan_import};
use yaoplm_core::journal::{self, Journal, TxProgress, TxState};
use yaoplm_core::plan::{Bytes, Expect, Plan, PlanKind, Step, Transaction};
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_device::fs::DeviceFs;
use yaoplm_device::{CancelToken, FsKind, RelPath};
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

/// FAT renames over a file in two steps: Windows removes the old journal,
/// then renames the new one in. A stick pulled in between leaves only the
/// complete, synced `.json.yaoplm-tmp`.
fn journal_left_as_temp_file(fs: &MemFs) -> String {
    let (path, _) = fs
        .files()
        .into_iter()
        .find(|(p, _)| p.starts_with(".yaoplm/journal/") && p.ends_with(".json"))
        .unwrap();
    let bytes = fs.bytes(&path).unwrap();
    DeviceFs::remove_file(fs, &RelPath::new(&path).unwrap()).unwrap();
    let tmp = format!("{path}.yaoplm-tmp");
    fs.insert_file(&tmp, &bytes);
    tmp
}

#[test]
fn a_journal_left_as_its_temp_file_is_still_recovered() {
    let (fs, sources, _) = interrupted_split();
    let tmp = journal_left_as_temp_file(&fs);

    // Scans see the unfinished operation, so nothing else may run.
    let blocked =
        plan_import(&fs, &sources, &[src("Huge.iso")], &ImportOptions::default()).unwrap();
    assert!(
        blocked
            .notices
            .iter()
            .any(|n| n.code == "unfinishedOperation"),
        "{:?}",
        blocked.notices
    );
    // The clean-up before a plan runs keeps it.
    journal::clean_temp(&fs).unwrap();
    assert_eq!(journals(&fs), 1, "{:?}", fs.files());

    let plan = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    assert!(plan.transactions[0].title.starts_with("Undo"), "{plan:?}");
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(ul_names(&fs).is_empty(), "{:?}", fs.files());
    assert_eq!(journals(&fs), 0);
    assert!(!fs.exists(&tmp));
}

#[test]
fn a_journal_left_as_its_temp_file_blocks_and_is_recovered_unclean() {
    // Without the clean-up first: recovery reads the temp file itself.
    let (fs, sources, _) = interrupted_split();
    let tmp = journal_left_as_temp_file(&fs);
    let plan = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    assert!(plan.transactions[0].title.starts_with("Undo"), "{plan:?}");
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(ul_names(&fs).is_empty(), "{:?}", fs.files());
    assert_eq!(journals(&fs), 0);
    assert!(!fs.exists(&tmp));
}

/// The journal the executor leaves when the device vanished while the only
/// transaction's first step ran: `Running`, nothing recorded as done, so
/// recovery undoes that step, which may or may not have happened.
fn left_running(fs: &MemFs, tx: Transaction) {
    let fingerprint = DeviceFs::info(fs).unwrap().fingerprint;
    let mut plan = Plan::new(PlanKind::Rename, "p", fingerprint, 0);
    plan.transactions.push(tx);
    let mut journal = Journal::new(&plan);
    journal.progress[0] = TxProgress {
        state: TxState::Running,
        done: 0,
    };
    journal::save(fs, &journal).unwrap();
}

fn roll_back(fs: &MemFs) -> yaoplm_core::exec::Report {
    let plan = plan_recovery(fs, RecoveryAction::Rollback).unwrap();
    let report = run(&plan, fs, &VirtualSources::new());
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(journals(fs), 0);
    report
}

fn rel(path: &str) -> RelPath {
    RelPath::new(path).unwrap()
}

#[test]
fn undo_puts_back_a_small_file_only_over_its_own_write() {
    let cases: [(&[u8], &[u8]); 3] = [
        (b"written", b"before"),
        // Changed since, by OPL or another manager: that change stays.
        (b"changed since", b"changed since"),
        // The write never happened.
        (b"before", b"before"),
    ];
    for (now, after) in cases {
        let fs = MemFs::new(FsKind::Fat32);
        let mut tx = Transaction::new("t");
        tx.push(Step::PutFile {
            path: rel("ul.cfg"),
            content: Bytes(b"written".to_vec()),
            expect: Expect::Content(Bytes(b"before".to_vec())),
        });
        left_running(&fs, tx);
        fs.insert_file("ul.cfg", now);
        let report = roll_back(&fs);
        assert_eq!(
            fs.bytes("ul.cfg").unwrap(),
            after,
            "{}",
            String::from_utf8_lossy(now)
        );
        let kept = report.notices.iter().any(|(_, n)| n.code == "notRestored");
        assert_eq!(kept, now == b"changed since", "{:?}", report.notices);
    }
}

#[test]
fn undo_removes_a_copy_only_when_it_is_one() {
    let cases: [(Option<&[u8]>, bool); 3] = [
        (Some(b"settings"), false),
        // The copy was refused: the user's own file was there.
        (Some(b"the user's own"), true),
        (None, false),
    ];
    for (target, kept) in cases {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("CFG/SLUS_200.62.cfg", b"settings");
        if let Some(bytes) = target {
            fs.insert_file("CFG/SLES_500.51.cfg", bytes);
        }
        let mut tx = Transaction::new("t");
        tx.push(Step::CopyFile {
            from: rel("CFG/SLUS_200.62.cfg"),
            to: rel("CFG/SLES_500.51.cfg"),
        });
        left_running(&fs, tx);
        roll_back(&fs);
        assert_eq!(fs.exists("CFG/SLES_500.51.cfg"), kept, "{target:?}");
        if kept {
            assert_eq!(fs.bytes("CFG/SLES_500.51.cfg").unwrap(), b"the user's own");
        }
        assert_eq!(fs.bytes("CFG/SLUS_200.62.cfg").unwrap(), b"settings");
    }
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
fn a_resumed_recovery_drops_games_bin_even_when_its_carrier_rolls_back() {
    // Two DVD games on exFAT and OPL's cache of the folder.
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file("DVD/games.bin", &[0u8; 188]);
    let sources = VirtualSources::new();
    sources.add_iso(src("First.iso"), big_iso("SLUS_212.18", 2 * GIB));
    sources.add_iso(src("Second.iso"), big_iso("SLUS_200.00", GIB));
    let inputs = [src("First.iso"), src("Second.iso")];
    let plan = plan_import(&fs, &sources, &inputs, &ImportOptions::default()).unwrap();
    let carriers = plan
        .transactions
        .iter()
        .filter(|t| {
            t.steps
                .iter()
                .any(|s| matches!(s, Step::Remove { path, .. } if path.as_str() == "DVD/games.bin"))
        })
        .count();
    assert_eq!(carriers, 1, "the drop rides with the first game");

    // Pulled out while the first game is written: the drop has not run.
    fs.unplug_after(GIB);
    assert!(!run(&plan, &fs, &sources).succeeded());
    fs.replug();
    assert!(fs.exists("DVD/games.bin"));

    // On resume the first game fails every time and is rolled back; the
    // second one is written all the same.
    let plan = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    fs.fail_once("create", "SLUS_212.18");
    let report = run(&plan, &fs, &sources);
    assert!(
        report.outcomes.contains(&Outcome::RolledBack),
        "{:?}",
        report.outcomes
    );
    assert!(fs.exists("DVD/SLUS_200.00.Second.iso"), "{:?}", fs.files());
    assert!(!fs.exists("DVD/SLUS_212.18.First.iso"));
    // The folder changed, so OPL's stale list must go (IMP-8).
    assert!(!fs.exists("DVD/games.bin"), "{:?}", fs.files());
    assert_eq!(journals(&fs), 0);
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
