//! Dump checks while adding games (GAME-5): the copy hashes what it writes.
//! The image hashes equal the ISO's, whatever the source and the layout on
//! the device; a raw CD dump also yields the hashes of its data track, the
//! bytes Redump hashes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use common::{assert_same_as, big_iso, run, run_with, src};
use yaoplm_core::checks::Checked;
use yaoplm_core::exec::{Context, Event, Outcome, execute_with_events};
use yaoplm_core::import::{ImportOptions, Layout, plan_import};
use yaoplm_core::plan::{Check, Plan, Step};
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_device::{CancelToken, FsKind, ReadSeek, SourceFs, SourceMeta};
use yaoplm_formats::iso9660::SectorSource;
use yaoplm_formats::zso;
use yaoplm_hash::{Algorithms, Hashes, hash_reader};
use yaoplm_test_support::bincue::{RawMode, to_bin};
use yaoplm_test_support::packed::{chd_cd, chd_dvd};
use yaoplm_test_support::{Content, MemFs, Variant, VirtualIso, VirtualSources};

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

/// Copies `packed` as it is and checks that it yields no hashes.
fn copied_unhashed(packed: &[u8]) {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_file(src("Game.zso"), Content::Bytes(packed.to_vec()));
    let plan = plan_import(&fs, &sources, &[src("Game.zso")], &checking()).unwrap();
    assert_eq!(plan.sources[0].check, Some(Check::Zso));
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(report.checked.is_empty(), "checked later by reading it");
    assert_eq!(fs.bytes("DVD/SLUS_212.18.Game.zso").unwrap(), packed);
}

#[test]
fn a_zso_with_a_broken_block_is_copied_unhashed() {
    // Index entries 0 and 1 swapped: block 0 ends before it starts, so
    // neither OPL nor the stream decoder reads it. The copy stays exact.
    let mut packed = zso_of(&Variant::Dvd9.build().to_vec(), 0);
    let (a, b) = (24..28, 28..32);
    let first: Vec<u8> = packed[a.clone()].to_vec();
    packed.copy_within(b.clone(), a.start);
    packed[b].copy_from_slice(&first);
    copied_unhashed(&packed);
}

#[test]
fn a_zso_the_stream_cannot_follow_is_copied_unhashed() {
    // Block 0 stored plain over the index, at byte 24: OPL seeks back for
    // it, the stream decoder is already past it.
    const PLAIN: u32 = 0x8000_0000;
    let packed = zso_of(&Variant::Dvd9.build().to_vec(), 0);
    let blocks = zso::ZsoHeader::parse(packed[..24].try_into().unwrap())
        .unwrap()
        .block_count() as usize;
    let index: Vec<u32> = packed[24..24 + (blocks + 1) * 4]
        .chunks(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let old_start = index[1] & !PLAIN;
    let new_start = (24 + 2048).max(24 + index.len() * 4) as u32;
    let mut moved = packed[..24].to_vec();
    moved.extend_from_slice(&(24 | PLAIN).to_le_bytes());
    for entry in &index[1..] {
        let shifted = (entry & !PLAIN) - old_start + new_start;
        moved.extend_from_slice(&(shifted | (entry & PLAIN)).to_le_bytes());
    }
    moved.resize(new_start as usize, 0);
    moved.extend_from_slice(&packed[old_start as usize..]);

    let mut reader = zso::ZsoReader::new(Cursor::new(moved.clone())).unwrap();
    let mut sector = [0u8; 2048];
    reader.read_sector(0, &mut sector).unwrap();
    assert_eq!(sector[..], moved[24..24 + 2048], "OPL reads it");
    copied_unhashed(&moved);
}

/// What happens once a source has been read up to a point.
#[derive(Clone)]
enum Trap {
    /// The user presses Cancel.
    Cancel(CancelToken),
    /// The disk holding it goes away.
    Fail,
}

/// `inner`, where reading `path` trips `trap` from byte `at` on.
struct Trapped<'a> {
    inner: &'a VirtualSources,
    path: PathBuf,
    at: u64,
    trap: Trap,
}

impl SourceFs for Trapped<'_> {
    fn metadata(&self, path: &Path) -> io::Result<SourceMeta> {
        self.inner.metadata(path)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        self.inner.list(dir)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        let inner = self.inner.open(path)?;
        if path != self.path {
            return Ok(inner);
        }
        Ok(Box::new(TrapReader {
            inner,
            pos: 0,
            at: self.at,
            trap: self.trap.clone(),
        }))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.inner.remove(path)
    }
}

struct TrapReader {
    inner: Box<dyn ReadSeek>,
    pos: u64,
    at: u64,
    trap: Trap,
}

impl Read for TrapReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.at {
            match &self.trap {
                Trap::Cancel(token) => token.cancel(),
                Trap::Fail => return Err(io::Error::other("cable pulled")),
            }
        }
        let n = self.inner.read(buf)?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for TrapReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.pos = self.inner.seek(to)?;
        Ok(self.pos)
    }
}

/// A game of several copy blocks, so its copy can stop half way.
fn several_blocks() -> VirtualIso {
    big_iso("SLUS_212.18", 32 << 20)
}

/// `A.iso`, then `B.iso` whose read trips `trap` well before its end.
fn import_two_with_a_trap(trap: Trap, cancel: &CancelToken) -> (Plan, yaoplm_core::exec::Report) {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_file(
        src("A.iso"),
        Content::Bytes(Variant::Retail.build().to_vec()),
    );
    sources.add_iso(src("B.iso"), several_blocks());
    let plan = plan_import(&fs, &sources, &[src("A.iso"), src("B.iso")], &checking()).unwrap();
    let trapped = Trapped {
        inner: &sources,
        path: src("B.iso"),
        at: 8 << 20,
        trap,
    };
    let report = run_with(&plan, &fs, &trapped, cancel, &mut |_| {});
    assert!(
        !common::files(&fs).iter().any(|(p, _)| p.contains(".B.")),
        "{:?}",
        fs.files()
    );
    (plan, report)
}

/// The transaction that copies the game from `name`.
fn game_of(plan: &Plan, name: &str) -> usize {
    plan.transactions
        .iter()
        .position(|t| t.game.is_some() && t.title.contains(name))
        .unwrap()
}

#[test]
fn a_cancelled_copy_yields_no_hashes() {
    let cancel = CancelToken::new();
    let (plan, report) = import_two_with_a_trap(Trap::Cancel(cancel.clone()), &cancel);
    assert!(report.cancelled, "{:?}", report.error);
    assert_eq!(report.outcomes[game_of(&plan, ".B.")], Outcome::RolledBack);
    // Only the game that went through whole.
    let checked: Vec<usize> = report.checked.iter().map(|(t, _)| *t).collect();
    assert_eq!(checked, [game_of(&plan, ".A.")]);
}

#[test]
fn a_failed_source_read_yields_no_hashes() {
    let (plan, report) = import_two_with_a_trap(Trap::Fail, &CancelToken::new());
    let err = report.error.as_ref().expect("the read failed");
    assert!(format!("{err:?}").contains("cable pulled"), "{err:?}");
    assert_eq!(report.outcomes[game_of(&plan, ".B.")], Outcome::RolledBack);
    let checked: Vec<usize> = report.checked.iter().map(|(t, _)| *t).collect();
    assert_eq!(checked, [game_of(&plan, ".A.")]);
}

/// Where the plan's first `WriteFile` puts its file.
fn written(plan: &Plan) -> String {
    plan.transactions
        .iter()
        .flat_map(|t| &t.steps)
        .find_map(|s| match s {
            Step::WriteFile { path, .. } => Some(path.to_string()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn a_resumed_copy_yields_no_hashes() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = several_blocks();
    sources.add_iso(src("Game.iso"), iso.clone());
    let plan = plan_import(&fs, &sources, &[src("Game.iso")], &checking()).unwrap();
    fs.unplug_after(16 << 20);
    let report = run(&plan, &fs, &sources);
    assert!(!report.succeeded());
    assert!(report.checked.is_empty());
    fs.replug();

    let recovery = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    assert!(
        recovery
            .sources
            .iter()
            .any(|s| s.check == Some(Check::Image)),
        "the journal keeps the check"
    );
    let report = run(&recovery, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(report.checked.is_empty(), "checked later by reading it");
    assert_same_as(&fs, &written(&plan), &iso, 0);
}

#[test]
fn a_ul_source_reopened_past_its_start_yields_no_hashes() {
    // The chunks written last first: the stream opens at 1 GiB, and the
    // pass from byte 0 stops where the first chunk ends.
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    sources.add_iso(
        src("Big.iso"),
        big_iso("SLUS_212.18", common::GIB + (8 << 20)),
    );
    let options = ImportOptions {
        layout: Layout::Ul,
        ..checking()
    };
    let mut plan = plan_import(&fs, &sources, &[src("Big.iso")], &options).unwrap();
    let tx = plan
        .transactions
        .iter_mut()
        .find(|t| t.game.is_some())
        .unwrap();
    let chunks: Vec<usize> = tx
        .steps
        .iter()
        .enumerate()
        .filter(|(_, s)| matches!(s, Step::WriteFile { .. }))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(chunks.len(), 2);
    tx.steps.swap(chunks[0], chunks[1]);
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(report.checked.is_empty(), "{:?}", report.checked);
}
