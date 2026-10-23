//! Import from CHD, zip and 7z, and from BIN/CUE with audio tracks
//! (IMP-1b): the ISO on the device is the data track, byte for byte.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{run, src};
use yaoplm_core::Media;
use yaoplm_core::archives::ArchiveSources;
use yaoplm_core::import::{ImportOptions, Transfer, plan_import};
use yaoplm_core::plan::Step;
use yaoplm_device::FsKind;
use yaoplm_test_support::bincue::{RawMode, cue_sheet, to_bin};
use yaoplm_test_support::packed::{chd_cd, chd_dvd, seven_zip, zip};
use yaoplm_test_support::{Content, MemFs, Variant, VirtualSources};

fn options() -> ImportOptions {
    ImportOptions::default()
}

fn codes(plan: &yaoplm_core::plan::Plan) -> Vec<String> {
    plan.transactions
        .iter()
        .flat_map(|t| &t.notices)
        .map(|n| n.code.clone())
        .collect()
}

#[test]
fn chd_dvd_and_cd() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let dvd = Variant::Dvd9.build().to_vec();
    let cd = Variant::Retail.build().to_vec();
    sources.add_file(src("Dvd Game.chd"), Content::Bytes(chd_dvd(&dvd).unwrap()));
    sources.add_file(
        src("Cd Game.chd"),
        Content::Bytes(chd_cd(&cd, RawMode::Mode2_2352, 300).unwrap()),
    );
    let plan = plan_import(
        &fs,
        &sources,
        &[src("Dvd Game.chd"), src("Cd Game.chd")],
        &options(),
    )
    .unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    let games: Vec<_> = plan
        .transactions
        .iter()
        .filter_map(|t| t.game.clone())
        .collect();
    assert_eq!(games.len(), 2);
    let cd_game = games.iter().find(|g| g.startup == "SLUS_200.62").unwrap();
    assert_eq!(cd_game.media, Media::Cd, "a CD CHD is a CD");
    assert!(codes(&plan).contains(&"audioTracksDropped".to_owned()));
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Cd Game.iso").unwrap(), cd);
    assert_eq!(fs.bytes("DVD/SLUS_212.18.Dvd Game.iso").unwrap(), dvd);
}

#[test]
fn redump_style_multi_track_bin_cue() {
    let fs = MemFs::new(FsKind::Fat32);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    sources.add_file(
        src("Game (Track 1).bin"),
        Content::Bytes(to_bin(&iso, RawMode::Mode2_2352, 0)),
    );
    sources.add_file(
        src("Game (Track 2).bin"),
        Content::Bytes(vec![0u8; 2352 * 40]),
    );
    sources.add_file(
        src("Game.cue"),
        Content::Bytes(
            b"FILE \"Game (Track 1).bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\nFILE \"Game (Track 2).bin\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n"
                .to_vec(),
        ),
    );
    let moved = ImportOptions {
        transfer: Transfer::Move,
        ..options()
    };
    // The folder: the BINs come in through the sheet only.
    let plan = plan_import(&fs, &sources, &[src("")], &moved).unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    assert!(codes(&plan).contains(&"audioTracksDropped".to_owned()));
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Game.iso").unwrap(), iso);
    // Moving deletes the whole dump, audio track included.
    let mut removed = sources.removed();
    removed.sort();
    assert_eq!(
        removed,
        [
            src("Game (Track 1).bin"),
            src("Game (Track 2).bin"),
            src("Game.cue")
        ]
    );
}

#[test]
fn one_bin_with_every_track() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::LfOnly.build().to_vec();
    let mut bin = to_bin(&iso, RawMode::Mode1_2352, 0);
    let data_sectors = bin.len() / 2352;
    bin.extend(vec![0x55u8; 2352 * 30]);
    sources.add_file(src("All.bin"), Content::Bytes(bin));
    let index = |sector: usize| {
        format!(
            "{:02}:{:02}:{:02}",
            sector / 75 / 60,
            sector / 75 % 60,
            sector % 75
        )
    };
    sources.add_file(
        src("All.cue"),
        Content::Bytes(
            format!(
                "FILE \"All.bin\" BINARY\n TRACK 01 MODE1/2352\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 00 {}\n  INDEX 01 {}\n",
                index(data_sectors),
                index(data_sectors + 10)
            )
            .into_bytes(),
        ),
    );
    let plan = plan_import(&fs, &sources, &[src("All.cue")], &options()).unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    assert!(run(&plan, &fs, &sources).succeeded());
    // Only the data track; the audio after it is not in the ISO.
    assert_eq!(fs.bytes("CD/SCES_500.03.All.iso").unwrap(), iso);
}

#[test]
fn zip_and_seven_zip_members() {
    let fs = MemFs::new(FsKind::ExFat);
    let host = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    let other = Variant::LfOnly.build().to_vec();
    host.add_file(
        src("Zipped.zip"),
        Content::Bytes(zip(&[("Zipped Game.iso", &iso), ("readme.txt", b"hi")], false).unwrap()),
    );
    let bin = to_bin(&other, RawMode::Mode1_2048, 0);
    let cue = cue_sheet("Packed.bin", RawMode::Mode1_2048, 0);
    host.add_file(
        src("Packed.7z"),
        Content::Bytes(
            seven_zip(&[
                ("Packed/Packed.cue", cue.as_bytes()),
                ("Packed/Packed.bin", &bin),
            ])
            .unwrap(),
        ),
    );
    let sources = ArchiveSources::new(&host);
    let moved = ImportOptions {
        transfer: Transfer::Move,
        ..options()
    };
    let plan = plan_import(&fs, &sources, &[src("")], &moved).unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    assert_eq!(
        plan.transactions
            .iter()
            .filter(|t| t.game.is_some())
            .count(),
        2,
        "{:?}",
        plan.skipped
    );
    let notices = codes(&plan);
    assert!(notices.contains(&"fromArchive".to_owned()));
    assert!(notices.contains(&"archiveKept".to_owned()));
    // Archives are never deleted, even in move mode.
    assert!(
        !plan
            .transactions
            .iter()
            .flat_map(|t| &t.steps)
            .any(|s| matches!(s, Step::RemoveSource { .. }))
    );
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Zipped Game.iso").unwrap(), iso);
    assert_eq!(fs.bytes("CD/SCES_500.03.Packed.iso").unwrap(), other);
    assert!(host.removed().is_empty());
}
