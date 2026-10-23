//! Converting games on the device between ISO, ZSO and `ul` (GAME-4):
//! round trips that keep every byte, the old image deleted only after the
//! commit point, rollback and recovery, FAT32 and profile limits.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Cursor;

use common::{big_iso, files, run, run_with};
use yaoplm_core::convert::{ConvertOptions, Target, plan_convert};
use yaoplm_core::exec::Outcome;
use yaoplm_core::ops::GameRef;
use yaoplm_core::plan::{PlanKind, Removal, Step};
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_device::{CancelToken, FsKind, RelPath};
use yaoplm_formats::LoaderProfile;
use yaoplm_formats::image::DiscImage;
use yaoplm_formats::iso_name::NameStyle;
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_formats::{Media, zso};
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

fn image(path: &str) -> GameRef {
    GameRef::Image {
        path: RelPath::new(path).unwrap(),
    }
}

fn ul(index: usize, startup: &str, name: &str) -> GameRef {
    GameRef::Ul {
        index,
        startup: startup.into(),
        name: name.into(),
    }
}

fn paths(fs: &MemFs) -> Vec<String> {
    files(fs).into_iter().map(|(p, _)| p).collect()
}

/// The ISO sectors of a file on the device, ZSO or not.
fn iso_bytes(fs: &MemFs, path: &str) -> Vec<u8> {
    let mut image = DiscImage::open(Cursor::new(fs.bytes(path).unwrap())).unwrap();
    let mut out = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    for lba in 0..image.sector_count() {
        image.read_sector(lba, &mut sector).unwrap();
        out.extend_from_slice(&sector);
    }
    out
}

fn options() -> ConvertOptions {
    ConvertOptions::default()
}

fn stick(kind: FsKind) -> (MemFs, Vec<u8>) {
    let fs = MemFs::new(kind);
    let iso = Variant::Retail.build().to_vec();
    fs.insert_file("CD/SLUS_200.62.Retail.iso", &iso);
    fs.insert_file("CD/games.bin", &[0u8; 188]);
    fs.insert_file("CFG/SLUS_200.62.cfg", b"$Compatibility=3\r\n");
    (fs, iso)
}

#[test]
fn iso_to_zso_and_back_keeps_every_byte() {
    let (fs, iso) = stick(FsKind::ExFat);
    let sources = VirtualSources::new();
    let plan = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Zso,
        &options(),
    )
    .unwrap();
    assert_eq!(plan.kind, PlanKind::Convert);
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert!(plan.deletes_user_data());
    let tx = &plan.transactions[0];
    assert!(
        matches!(&tx.steps[0], Step::WriteZso { path, .. } if path.as_str() == "CD/SLUS_200.62.Retail.zso")
    );
    // The ISO goes only after the ZSO is complete.
    assert_eq!(tx.commit, 1);
    assert!(matches!(
        &tx.steps[1],
        Step::Remove {
            what: Removal::UserData,
            ..
        }
    ));
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(
        paths(&fs),
        ["CD/SLUS_200.62.Retail.zso", "CFG/SLUS_200.62.cfg"]
    );
    let packed = fs.bytes("CD/SLUS_200.62.Retail.zso").unwrap();
    assert_eq!(&packed[..4], b"ZISO");
    assert!(packed.len() < iso.len());
    assert_eq!(iso_bytes(&fs, "CD/SLUS_200.62.Retail.zso"), iso);

    let back = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.zso")],
        Target::Iso,
        &options(),
    )
    .unwrap();
    assert!(back.is_executable(), "{:?}", back.notices);
    assert!(run(&back, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Retail.iso").unwrap(), iso);
    assert!(!fs.exists("CD/SLUS_200.62.Retail.zso"));
    assert_eq!(
        fs.bytes("CFG/SLUS_200.62.cfg").unwrap(),
        b"$Compatibility=3\r\n"
    );
}

#[test]
fn iso_to_ul_and_back() {
    let (fs, iso) = stick(FsKind::Fat32);
    let sources = VirtualSources::new();
    let plan = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Ul,
        &options(),
    )
    .unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert!(run(&plan, &fs, &sources).succeeded());
    let table = UlCfg::parse(&fs.bytes("ul.cfg").unwrap());
    assert_eq!(table.records.len(), 1);
    let record = &table.records[0];
    assert_eq!(record.name(), "Retail");
    assert_eq!(record.startup(), "SLUS_200.62");
    assert_eq!(record.media(), Media::Cd);
    assert_eq!(fs.bytes(&record.chunk_name(0)).unwrap(), iso);
    assert!(!fs.exists("CD/SLUS_200.62.Retail.iso"));

    let back = plan_convert(
        &fs,
        &[ul(0, "SLUS_200.62", "Retail")],
        Target::Iso,
        &options(),
    )
    .unwrap();
    assert!(back.is_executable(), "{:?}", back.notices);
    let tx = &back.transactions[0];
    // Image and ul.cfg before the commit point, the chunk after it.
    assert_eq!(tx.commit, 2);
    assert!(run(&back, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Retail.iso").unwrap(), iso);
    assert_eq!(fs.bytes("ul.cfg").unwrap(), Vec::<u8>::new());
    assert!(
        !paths(&fs)
            .iter()
            .any(|p| p.starts_with("ul.") && p != "ul.cfg")
    );
}

#[test]
fn two_ul_games_to_zso_in_one_plan() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let a = Variant::Retail.build().to_vec();
    let b = Variant::LfOnly.build().to_vec();
    let ra = UlRecord::new("Game A", "SLUS_200.62", 1, Media::Dvd).unwrap();
    let rb = UlRecord::new("Game B", "SCES_500.03", 1, Media::Cd).unwrap();
    fs.insert_file(&ra.chunk_name(0), &a);
    fs.insert_file(&rb.chunk_name(0), &b);
    let mut table = ra.as_bytes().to_vec();
    table.extend_from_slice(rb.as_bytes());
    table.extend_from_slice(b"ragged");
    fs.insert_file("ul.cfg", &table);

    let plan = plan_convert(
        &fs,
        &[
            ul(0, "SLUS_200.62", "Game A"),
            ul(1, "SCES_500.03", "Game B"),
        ],
        Target::Zso,
        &options(),
    )
    .unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert_eq!(plan.transactions.len(), 2);
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(iso_bytes(&fs, "DVD/SLUS_200.62.Game A.zso"), a);
    assert_eq!(iso_bytes(&fs, "CD/SCES_500.03.Game B.zso"), b);
    // Both records are gone; the ragged tail stays.
    assert_eq!(fs.bytes("ul.cfg").unwrap(), b"ragged");
}

#[test]
fn a_failed_compression_leaves_the_iso() {
    let (fs, iso) = stick(FsKind::ExFat);
    let sources = VirtualSources::new();
    let plan = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Zso,
        &options(),
    )
    .unwrap();
    fs.fail_once("rename", "Retail.zso");
    let report = run(&plan, &fs, &sources);
    assert!(!report.succeeded());
    assert_eq!(report.outcomes, [Outcome::RolledBack]);
    assert_eq!(fs.bytes("CD/SLUS_200.62.Retail.iso").unwrap(), iso);
    assert!(
        !paths(&fs).iter().any(|p| p.contains(".zso")),
        "{:?}",
        paths(&fs)
    );
}

#[test]
fn a_crash_after_the_commit_finishes_the_deletion() {
    let (fs, _) = stick(FsKind::ExFat);
    let sources = VirtualSources::new();
    let plan = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Zso,
        &options(),
    )
    .unwrap();
    fs.fail_once("remove", "Retail.iso");
    let report = run(&plan, &fs, &sources);
    assert_eq!(report.outcomes, [Outcome::Interrupted]);
    assert!(fs.exists("CD/SLUS_200.62.Retail.zso"));
    assert!(fs.exists("CD/SLUS_200.62.Retail.iso"));

    // Even "roll back" finishes it: OPL may already list the ZSO.
    let recovery = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    assert!(run(&recovery, &fs, &sources).succeeded());
    assert_eq!(
        paths(&fs),
        ["CD/SLUS_200.62.Retail.zso", "CFG/SLUS_200.62.cfg"]
    );
}

#[test]
fn what_cannot_be_converted_is_skipped_with_a_reason() {
    let (fs, _) = stick(FsKind::Fat32);
    let opl11 = ConvertOptions {
        profile: LoaderProfile::Opl110,
        ..ConvertOptions::default()
    };
    let plan = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Zso,
        &opl11,
    )
    .unwrap();
    assert!(!plan.is_executable());
    assert_eq!(plan.skipped[0].reason.code, "zsoUnsupportedByProfile");
    assert!(plan.notices.iter().any(|n| n.code == "nothingToConvert"));

    let same = plan_convert(
        &fs,
        &[image("CD/SLUS_200.62.Retail.iso")],
        Target::Iso,
        &options(),
    )
    .unwrap();
    assert_eq!(same.skipped[0].reason.code, "alreadyThatFormat");

    // A ZSO of a 5 GiB disc cannot become an ISO on FAT32.
    let header = zso::ZsoHeader {
        header_size: 24,
        total_bytes: 5 << 30,
        block_size: 2048,
        version: 1,
        align: 2,
        reserved: [0; 2],
    };
    fs.insert_file("DVD/SLUS_212.18.Huge.zso", &header.to_bytes());
    let huge = plan_convert(
        &fs,
        &[image("DVD/SLUS_212.18.Huge.zso")],
        Target::Iso,
        &options(),
    )
    .unwrap();
    // From ZSO the advice is to convert to ul instead.
    assert_eq!(huge.skipped[0].reason.code, "tooLargeUseUl");

    let missing = plan_convert(&fs, &[image("CD/Nope.iso")], Target::Zso, &options()).unwrap();
    assert_eq!(missing.skipped[0].reason.code, "notFound");
}

#[test]
fn bulk_conversion_counts_freed_space() {
    // Two incompressible 6 MiB images on a stick with room for one ZSO:
    // converting both at once fits, because each ISO is gone before the
    // next ZSO is written.
    let a = big_iso("SLUS_200.62", 6 << 20).to_vec();
    let b = big_iso("SCES_500.03", 6 << 20).to_vec();
    // MemFs counts the images against its free space.
    let isos = (a.len() + b.len()) as u64;
    let stick_with = |free: u64| {
        let fs = MemFs::new(FsKind::ExFat).with_free_bytes(isos + free);
        fs.insert_file("CD/SLUS_200.62.A.iso", &a);
        fs.insert_file("CD/SCES_500.03.B.iso", &b);
        fs
    };
    let games = [image("CD/SLUS_200.62.A.iso"), image("CD/SCES_500.03.B.iso")];
    let roomy = plan_convert(&stick_with(1 << 40), &games, Target::Zso, &options()).unwrap();
    let total = roomy.write_bytes();
    let peak = roomy.peak_write_bytes();
    let free = peak + (4 << 20);
    assert!(
        free < total,
        "the stick must be too small for both ZSOs at once"
    );

    let fs = stick_with(free);
    let plan = plan_convert(&fs, &games, Target::Zso, &options()).unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(iso_bytes(&fs, "CD/SCES_500.03.B.zso"), b);

    let tight = plan_convert(&stick_with(peak / 2), &games, Target::Zso, &options()).unwrap();
    assert!(tight.notices.iter().any(|n| n.code == "noSpace"));
}

#[test]
fn a_cancelled_compression_leaves_the_iso() {
    let fs = MemFs::new(FsKind::ExFat);
    let iso = big_iso("SLUS_200.62", 12 << 20).to_vec();
    fs.insert_file("DVD/SLUS_200.62.Noise.iso", &iso);
    let plan = plan_convert(
        &fs,
        &[image("DVD/SLUS_200.62.Noise.iso")],
        Target::Zso,
        &options(),
    )
    .unwrap();
    let cancel = CancelToken::new();
    let report = run_with(&plan, &fs, &VirtualSources::new(), &cancel, &mut |p| {
        if p.done > 0 {
            cancel.cancel();
        }
    });
    assert!(report.cancelled);
    assert_eq!(report.outcomes, [Outcome::RolledBack]);
    assert_eq!(fs.bytes("DVD/SLUS_200.62.Noise.iso").unwrap(), iso);
    assert!(
        !paths(&fs).iter().any(|p| p.contains(".zso")),
        "{:?}",
        paths(&fs)
    );
}

#[test]
fn every_record_of_a_ul_game_goes_and_odd_chunks_are_refused() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    let cd = UlRecord::new("Game", "SLUS_200.62", 1, Media::Cd).unwrap();
    let dvd = UlRecord::new("Game", "SLUS_200.62", 1, Media::Dvd).unwrap();
    fs.insert_file(&cd.chunk_name(0), &iso);
    // The same game listed three times: all name the one chunk.
    let mut table = cd.as_bytes().to_vec();
    table.extend_from_slice(cd.as_bytes());
    table.extend_from_slice(dvd.as_bytes());
    fs.insert_file("ul.cfg", &table);
    let plan = plan_convert(
        &fs,
        &[ul(0, "SLUS_200.62", "Game")],
        Target::Iso,
        &options(),
    )
    .unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    assert!(
        plan.transactions[0]
            .notices
            .iter()
            .any(|n| n.code == "ulDuplicateRecords")
    );
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("ul.cfg").unwrap(), Vec::<u8>::new());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Game.iso").unwrap(), iso);

    // A first chunk shorter than 1 GiB: OPL reads part 1 at 1 GiB.
    let two = UlRecord::new("Two", "SCES_500.03", 2, Media::Dvd).unwrap();
    fs.insert_file(&two.chunk_name(0), &iso);
    fs.insert_file(&two.chunk_name(1), &iso);
    fs.insert_file("ul.cfg", two.as_bytes());
    let plan = plan_convert(&fs, &[ul(0, "SCES_500.03", "Two")], Target::Iso, &options()).unwrap();
    assert_eq!(plan.skipped[0].reason.code, "ulPartSize");
}

#[test]
fn an_id_fixed_in_ul_cfg_stays_in_the_name() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    // SYSTEM.CNF says SLUS_200.62.
    let iso = Variant::Retail.build().to_vec();
    let fixed = UlRecord::new("Fixed", "SLUS_999.99", 1, Media::Cd).unwrap();
    let plain = UlRecord::new("Plain", "SLUS_200.62", 1, Media::Cd).unwrap();
    fs.insert_file(&fixed.chunk_name(0), &iso);
    fs.insert_file(&plain.chunk_name(0), &iso);
    let mut table = fixed.as_bytes().to_vec();
    table.extend_from_slice(plain.as_bytes());
    fs.insert_file("ul.cfg", &table);
    let title_only = ConvertOptions {
        name_style: Some(NameStyle::TitleOnly),
        ..options()
    };
    let plan = plan_convert(
        &fs,
        &[ul(0, "SLUS_999.99", "Fixed"), ul(1, "SLUS_200.62", "Plain")],
        Target::Iso,
        &title_only,
    )
    .unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    let kept = |i: usize| {
        plan.transactions[i]
            .notices
            .iter()
            .any(|n| n.code == "idKeptInName")
    };
    assert!(kept(0));
    assert!(!kept(1));
    assert!(run(&plan, &fs, &sources).succeeded());
    assert!(fs.exists("CD/SLUS_999.99.Fixed.iso"), "{:?}", paths(&fs));
    assert!(fs.exists("CD/Plain.iso"), "{:?}", paths(&fs));
}

/// A ZSO conversion the stick vanished in the middle of, and a file of
/// the user's that took the ZSO's name meanwhile.
fn interrupted_compression() -> (MemFs, Vec<u8>) {
    let fs = MemFs::new(FsKind::ExFat);
    let iso = big_iso("SLUS_200.62", 6 << 20).to_vec();
    fs.insert_file("DVD/SLUS_200.62.Noise.iso", &iso);
    let plan = plan_convert(
        &fs,
        &[image("DVD/SLUS_200.62.Noise.iso")],
        Target::Zso,
        &options(),
    )
    .unwrap();
    fs.unplug_after(1 << 20);
    let report = run(&plan, &fs, &VirtualSources::new());
    assert_eq!(report.outcomes, [Outcome::Interrupted]);
    fs.replug();
    fs.insert_file("DVD/SLUS_200.62.Noise.zso", b"the user's own file");
    (fs, iso)
}

fn nothing_to_recover(fs: &MemFs) -> bool {
    plan_recovery(fs, RecoveryAction::Rollback)
        .unwrap()
        .notices
        .iter()
        .any(|n| n.code == "nothingToRecover")
}

#[test]
fn undoing_a_compression_leaves_what_it_did_not_write() {
    let (fs, iso) = interrupted_compression();
    let sources = VirtualSources::new();
    let undo = plan_recovery(&fs, RecoveryAction::Rollback).unwrap();
    let report = run(&undo, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(
        fs.bytes("DVD/SLUS_200.62.Noise.zso").unwrap(),
        b"the user's own file"
    );
    assert_eq!(fs.bytes("DVD/SLUS_200.62.Noise.iso").unwrap(), iso);
    assert!(
        !paths(&fs).iter().any(|p| p.ends_with("yaoplm-part")),
        "{:?}",
        paths(&fs)
    );
    assert!(nothing_to_recover(&fs));
}

#[test]
fn a_resume_that_cannot_finish_is_rolled_back_not_retried_for_ever() {
    let (fs, iso) = interrupted_compression();
    let sources = VirtualSources::new();
    // The user's file is no ZSO of this game: resuming must not take it for
    // one (the ISO would go next), and stops.
    let resume = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    assert!(!run(&resume, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("DVD/SLUS_200.62.Noise.iso").unwrap(), iso);
    // That game was rolled back; the next recovery only closes journals.
    let again = plan_recovery(&fs, RecoveryAction::Resume).unwrap();
    let report = run(&again, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert!(nothing_to_recover(&fs));
    assert_eq!(fs.bytes("DVD/SLUS_200.62.Noise.iso").unwrap(), iso);
    assert_eq!(
        fs.bytes("DVD/SLUS_200.62.Noise.zso").unwrap(),
        b"the user's own file"
    );
}
