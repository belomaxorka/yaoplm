//! Dump checks while adding games (GAME-5): the copy hashes what it writes.
//! The image hashes equal the ISO's, whatever the source and the layout on
//! the device; a raw CD dump also yields the hashes of its data track, the
//! bytes Redump hashes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Cursor;

use common::{run, src};
use yaoplm_core::checks::Checked;
use yaoplm_core::exec::{Context, Event, execute_with_events};
use yaoplm_core::import::{ImportOptions, Layout, plan_import};
use yaoplm_core::plan::{Check, Plan};
use yaoplm_device::{CancelToken, FsKind};
use yaoplm_formats::zso;
use yaoplm_hash::{Algorithms, Hashes, hash_reader};
use yaoplm_test_support::bincue::{RawMode, to_bin};
use yaoplm_test_support::packed::{chd_cd, chd_dvd};
use yaoplm_test_support::{Content, MemFs, Variant, VirtualSources};

fn checking() -> ImportOptions {
    ImportOptions {
        check: true,
        ..ImportOptions::default()
    }
}

fn hashes(bytes: &[u8]) -> Hashes {
    hash_reader(
        &mut Cursor::new(bytes),
        Algorithms::default(),
        &|| false,
        &mut |_| {},
    )
    .unwrap()
}

/// Imports `name` alone and returns what the copy hashed.
fn import_one(
    fs: &MemFs,
    sources: &VirtualSources,
    name: &str,
    options: &ImportOptions,
) -> Checked {
    let plan = plan_import(fs, sources, &[src(name)], options).unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    let report = run(&plan, fs, sources);
    assert!(report.succeeded(), "{:?}", report.error);
    let games: Vec<usize> = plan
        .transactions
        .iter()
        .enumerate()
        .filter(|(_, t)| t.game.is_some())
        .map(|(i, _)| i)
        .collect();
    assert_eq!(report.checked.len(), 1, "{:?}", report.checked);
    let (transaction, checked) = report.checked[0];
    assert_eq!(games, [transaction], "the hashes belong to the game");
    checked
}

#[test]
fn iso_hashes_match_the_written_image() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Dvd9.build().to_vec();
    sources.add_file(src("Game.iso"), Content::Bytes(iso.clone()));
    let checked = import_one(&fs, &sources, "Game.iso", &checking());
    assert_eq!(checked.image, hashes(&iso));
    assert_eq!(checked.track, None, "an ISO has no raw track");
}

#[test]
fn raw_bin_yields_the_track_hashes_redump_has() {
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    let bin = to_bin(&iso, RawMode::Mode2_2352, 0);
    sources.add_file(src("Game.bin"), Content::Bytes(bin.clone()));
    sources.add_file(
        src("Game.cue"),
        Content::Bytes(
            b"FILE \"Game.bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n"
                .to_vec(),
        ),
    );
    let checked = import_one(&fs, &sources, "Game.cue", &checking());
    assert_eq!(checked.image, hashes(&iso));
    assert_eq!(checked.track, Some(hashes(&bin)));
    assert_eq!(fs.bytes("CD/SLUS_200.62.Game.iso").unwrap(), iso);
}

#[test]
fn cooked_bin_has_no_track_hashes() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    sources.add_file(
        src("Game.bin"),
        Content::Bytes(to_bin(&iso, RawMode::Mode2_2336, 0)),
    );
    let checked = import_one(&fs, &sources, "Game.bin", &checking());
    assert_eq!(checked.image, hashes(&iso));
    assert_eq!(
        checked.track, None,
        "2336-byte sectors lack the sync and header"
    );
}

#[test]
fn track_hashes_stop_where_the_data_track_ends() {
    // One BIN with every track: the data track ends at the next INDEX 00.
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    let track1 = to_bin(&iso, RawMode::Mode2_2352, 0);
    let mut bin = track1.clone();
    bin.extend(std::iter::repeat_n(0x5Au8, 2352 * 300));
    let pregap = track1.len() / 2352;
    let cue = format!(
        "FILE \"Game.bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n  TRACK 02 AUDIO\r\n    INDEX 00 {}\r\n    INDEX 01 {}\r\n",
        msf(pregap),
        msf(pregap + 150)
    );
    sources.add_file(src("Game.bin"), Content::Bytes(bin));
    sources.add_file(src("Game.cue"), Content::Bytes(cue.into_bytes()));
    let checked = import_one(&fs, &sources, "Game.cue", &checking());
    assert_eq!(checked.track, Some(hashes(&track1)));
}

fn msf(frames: usize) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        frames / 75 / 60,
        frames / 75 % 60,
        frames % 75
    )
}

#[test]
fn chd_cd_and_dvd() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let cd = Variant::Retail.build().to_vec();
    sources.add_file(
        src("Cd.chd"),
        Content::Bytes(chd_cd(&cd, RawMode::Mode2_2352, 300).unwrap()),
    );
    let checked = import_one(&fs, &sources, "Cd.chd", &checking());
    assert_eq!(checked.image, hashes(&cd));
    // The raw track the CHD was made from, audio left out.
    let bin = to_bin(&cd, RawMode::Mode2_2352, 0);
    assert_eq!(checked.track, Some(hashes(&bin)));

    let fs = MemFs::new(FsKind::ExFat);
    let dvd = Variant::Dvd9.build().to_vec();
    sources.add_file(src("Dvd.chd"), Content::Bytes(chd_dvd(&dvd).unwrap()));
    let checked = import_one(&fs, &sources, "Dvd.chd", &checking());
    assert_eq!(checked.image, hashes(&dvd));
    assert_eq!(checked.track, None);
}

fn zso_of(iso: &[u8], align: u8) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    zso::compress_aligned(
        &mut Cursor::new(iso),
        iso.len() as u64,
        &mut out,
        align,
        &mut |_| {},
    )
    .unwrap();
    out.into_inner()
}

#[test]
fn zso_kept_as_is_hashes_the_iso_inside() {
    let iso = Variant::Dvd9.build().to_vec();
    for align in [0, 3] {
        let fs = MemFs::new(FsKind::ExFat);
        let sources = VirtualSources::new();
        let packed = zso_of(&iso, align);
        sources.add_file(src("Game.zso"), Content::Bytes(packed.clone()));
        let plan = plan_import(&fs, &sources, &[src("Game.zso")], &checking()).unwrap();
        assert_eq!(plan.sources[0].check, Some(Check::Zso));
        let checked = import_one(&fs, &sources, "Game.zso", &checking());
        assert_eq!(checked.image, hashes(&iso), "align {align}");
        assert_eq!(fs.bytes("DVD/SLUS_212.18.Game.zso").unwrap(), packed);
    }
}

#[test]
fn compressing_hashes_the_iso_before_compression() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    let bin = to_bin(&iso, RawMode::Mode2_2352, 0);
    sources.add_file(src("Game.bin"), Content::Bytes(bin.clone()));
    let options = ImportOptions {
        compress: true,
        ..checking()
    };
    let checked = import_one(&fs, &sources, "Game.bin", &options);
    assert_eq!(checked.image, hashes(&iso));
    assert_eq!(checked.track, Some(hashes(&bin)));
    assert!(fs.bytes("CD/SLUS_200.62.Game.zso").is_some());
}

#[test]
fn ul_chunks_are_hashed_as_one_image() {
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    let iso = Variant::Dvd9.build().to_vec();
    sources.add_file(src("Game.iso"), Content::Bytes(iso.clone()));
    let options = ImportOptions {
        layout: Layout::Ul,
        ..checking()
    };
    let checked = import_one(&fs, &sources, "Game.iso", &options);
    assert_eq!(checked.image, hashes(&iso));
}

#[test]
fn ul_game_is_hashed_across_its_chunks() {
    // Two chunks read through one stream.
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    let iso = common::big_iso("SLUS_212.18", common::GIB + 100 * (1 << 20));
    sources.add_iso(src("Big.iso"), iso.clone());
    let options = ImportOptions {
        layout: Layout::Ul,
        ..checking()
    };
    let checked = import_one(&fs, &sources, "Big.iso", &options);
    let mut reader = iso.clone();
    let expected = hash_reader(&mut reader, Algorithms::default(), &|| false, &mut |_| {}).unwrap();
    assert_eq!(checked.image, expected);
}

#[test]
fn checks_are_opt_in_and_reported_once_done() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    sources.add_file(src("A.iso"), Content::Bytes(iso.clone()));
    sources.add_file(src("B.iso"), Content::Bytes(Variant::Dvd9.build().to_vec()));

    let plan = plan_import(&fs, &sources, &[src("A.iso")], &ImportOptions::default()).unwrap();
    assert!(plan.sources.iter().all(|s| s.check.is_none()));
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded());
    assert!(report.checked.is_empty());

    // The event comes after the transaction is done, with its index.
    let fs = MemFs::new(FsKind::ExFat);
    let plan = plan_import(&fs, &sources, &[src("A.iso"), src("B.iso")], &checking()).unwrap();
    let mut seen = Vec::new();
    let mut done = Vec::new();
    let report = execute_with_events(
        &plan,
        &Context {
            fs: &fs,
            sources: &sources,
            cancel: &CancelToken::new(),
        },
        &mut |event| match event {
            Event::Checked {
                transaction,
                source,
                checked,
            } => {
                assert!(done.contains(&transaction), "after the transaction");
                seen.push((transaction, source, checked.image.size));
            }
            Event::Transaction {
                index,
                state: yaoplm_core::exec::TxEvent::Done,
                ..
            } => done.push(index),
            _ => {}
        },
    );
    assert!(report.succeeded());
    assert_eq!(seen.len(), 2);
    assert_eq!(report.checked.len(), 2);
    for (transaction, source, _) in &seen {
        let tx = &plan.transactions[*transaction];
        assert!(tx.steps.iter().any(|s| matches!(
            s,
            yaoplm_core::plan::Step::WriteFile { source: s, .. } if s == source
        )));
    }
}

#[test]
fn journals_without_checks_still_load() {
    // A plan saved before the field existed.
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_file(
        src("A.iso"),
        Content::Bytes(Variant::Retail.build().to_vec()),
    );
    let plan = plan_import(&fs, &sources, &[src("A.iso")], &checking()).unwrap();
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains(r#""check":"image""#), "{json}");
    let old = json.replace(r#","check":"image""#, "");
    let back: Plan = serde_json::from_str(&old).unwrap();
    assert_eq!(back.sources[0].check, None);
}

#[test]
fn a_zso_the_stream_cannot_follow_is_copied_unhashed() {
    // Blocks 0 and 1 swapped: OPL reads it, the stream decoder cannot.
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Dvd9.build().to_vec();
    let mut packed = zso_of(&iso, 0);
    let (a, b) = (24..28, 28..32);
    let first: Vec<u8> = packed[a.clone()].to_vec();
    packed.copy_within(b.clone(), a.start);
    packed[b].copy_from_slice(&first);
    sources.add_file(src("Game.zso"), Content::Bytes(packed.clone()));
    let plan = plan_import(&fs, &sources, &[src("Game.zso")], &checking()).unwrap();
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(report.checked.is_empty(), "checked later by reading it");
    assert_eq!(fs.bytes("DVD/SLUS_212.18.Game.zso").unwrap(), packed);
}
