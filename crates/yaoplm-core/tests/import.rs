//! The import pipeline end to end: plan, execute, check the device
//! (IMP-1 to IMP-11, LIB-3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Cursor;

use common::{GIB, assert_same_as, big_iso, files, run, src};
use yaoplm_core::exec::Outcome;
use yaoplm_core::import::{ImportOptions, ImportOverride, Layout, Transfer, plan_import};
use yaoplm_core::plan::{Level, Plan, Step};
use yaoplm_core::{LoaderProfile, Media};
use yaoplm_device::FsKind;
use yaoplm_formats::iso_name::NameStyle;
use yaoplm_formats::ul_cfg::{CHUNK_SIZE, UlCfg};
use yaoplm_formats::usba_crc32::usba_crc32;
use yaoplm_formats::zso;
use yaoplm_test_support::bincue::{RawMode, cue_sheet, to_bin};
use yaoplm_test_support::{Content, FatImage, MemFs, Variant, VirtualSources};

fn options() -> ImportOptions {
    ImportOptions::default()
}

fn plan_for(
    fs: &MemFs,
    sources: &VirtualSources,
    inputs: &[&str],
    options: &ImportOptions,
) -> Plan {
    let inputs: Vec<_> = inputs.iter().map(|p| src(p)).collect();
    plan_import(fs, sources, &inputs, options).unwrap()
}

fn skipped_codes(plan: &Plan) -> Vec<&str> {
    plan.skipped
        .iter()
        .map(|s| s.reason.code.as_str())
        .collect()
}

#[test]
fn cd_and_dvd_images_land_in_their_folders() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    sources.add_iso(src("Big Game.iso"), Variant::Dvd9.build());
    let plan = plan_for(&fs, &sources, &["Retail.iso", "Big Game.iso"], &options());
    assert!(plan.is_executable(), "{:?}", plan.notices);
    assert!(plan.skipped.is_empty(), "{:?}", plan.skipped);
    // Folders first, then one transaction per game.
    assert_eq!(plan.transactions.len(), 3);
    assert_eq!(plan.transactions[0].steps.len(), 9);
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    assert_eq!(report.outcomes, [Outcome::Done; 3]);
    assert!(report.journal.is_none());

    let retail = Variant::Retail.build();
    assert_same_as(&fs, "CD/SLUS_200.62.Retail.iso", &retail, 0);
    assert_same_as(
        &fs,
        "DVD/SLUS_212.18.Big Game.iso",
        &Variant::Dvd9.build(),
        0,
    );
    for folder in ["CFG", "THM", "LNG", "ART", "VMC", "CHT", "APPS"] {
        assert!(fs.exists(folder), "{folder}");
    }
    // The journal is gone; only its folder stays.
    assert!(
        fs.files()
            .iter()
            .all(|(p, _)| !p.starts_with(".yaoplm/journal/"))
    );
    // Nothing is left half written.
    assert!(fs.files().iter().all(|(p, _)| !p.contains(".yaoplm-")));
}

#[test]
fn imports_into_a_fat32_image() {
    let dir = tempfile::tempdir().unwrap();
    let image = FatImage::create(&dir.path().join("fat32.img"), 48 << 20).unwrap();
    let sources = VirtualSources::new();
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    sources.add_iso(src("SCES_500.03.Lf Only.iso"), Variant::LfOnly.build());
    let inputs = [src("Retail.iso"), src("SCES_500.03.Lf Only.iso")];
    let plan = plan_import(&image, &sources, &inputs, &options()).unwrap();
    let report = run(&plan, &image, &sources);
    assert!(report.succeeded(), "{:?}", report.error);

    // Through a fresh mount: everything reached the image.
    image.remount().unwrap();
    assert_eq!(
        image.read_file("CD/SLUS_200.62.Retail.iso").unwrap(),
        Variant::Retail.build().to_vec()
    );
    assert_eq!(
        image.read_file("CD/SCES_500.03.Lf Only.iso").unwrap(),
        Variant::LfOnly.build().to_vec()
    );

    // ul on a real FAT32 implementation too, chunk names and all.
    let forced = ImportOptions {
        layout: Layout::Ul,
        overrides: vec![ImportOverride {
            path: src("Retail.iso"),
            title: Some("Retail Copy".into()),
            media: Some(Media::Dvd),
        }],
        ..options()
    };
    let plan = plan_import(&image, &sources, &inputs[..1], &forced).unwrap();
    assert!(
        plan.is_executable(),
        "{:?} {:?}",
        plan.notices,
        plan.skipped
    );
    assert!(run(&plan, &image, &sources).succeeded());
    image.remount().unwrap();
    let table = UlCfg::parse(&image.read_file("ul.cfg").unwrap());
    assert_eq!(table.records.len(), 1);
    let record = &table.records[0];
    assert_eq!(record.name(), "Retail Copy");
    assert_eq!(record.startup(), "SLUS_200.62");
    assert_eq!(record.media(), Media::Dvd);
    assert!(record.anomalies().is_empty());
    let chunk = format!("ul.{:08X}.SLUS_200.62.00", usba_crc32(b"Retail Copy"));
    assert_eq!(chunk, record.chunk_name(0));
    assert_eq!(
        image.read_file(&chunk).unwrap(),
        Variant::Retail.build().to_vec()
    );
}

#[test]
fn games_over_4_gib_become_ul_on_fat32_only() {
    let iso = big_iso("SLUS_212.18", 4_400_000_000);
    let len = iso.len();
    let sources = VirtualSources::new();
    sources.add_iso(src("Huge Game.iso"), iso.clone());

    let fat = MemFs::new(FsKind::Fat32);
    let plan = plan_for(&fat, &sources, &["Huge Game.iso"], &options());
    let tx = plan.transactions.last().unwrap();
    assert_eq!(tx.game.as_ref().unwrap().format, "UL");
    assert!(tx.notices.iter().any(|n| n.code == "storedAsUl"));
    assert_eq!(tx.game.as_ref().unwrap().media, Media::Dvd);
    let report = run(&plan, &fat, &sources);
    assert!(report.succeeded(), "{:?}", report.error);

    let crc = usba_crc32(b"Huge Game");
    let chunks: Vec<_> = files(&fat)
        .into_iter()
        .filter(|(p, _)| p.starts_with("ul.") && p != "ul.cfg")
        .collect();
    assert_eq!(chunks.len(), 5, "{chunks:?}");
    for (part, (name, size)) in chunks.iter().enumerate() {
        assert_eq!(*name, format!("ul.{crc:08X}.SLUS_212.18.{part:02x}"));
        let expected = (len - part as u64 * CHUNK_SIZE).min(CHUNK_SIZE);
        assert_eq!(*size, expected, "{name}");
        assert_same_as(&fat, name, &iso, part as u64 * CHUNK_SIZE);
    }
    let table = UlCfg::parse(&fat.bytes("ul.cfg").unwrap());
    assert_eq!(table.records[0].parts(), 5);
    assert_eq!(table.records[0].name(), "Huge Game");

    // exFAT keeps one image (IMP-6).
    let exfat = MemFs::new(FsKind::ExFat);
    let plan = plan_for(&exfat, &sources, &["Huge Game.iso"], &options());
    assert!(run(&plan, &exfat, &sources).succeeded());
    assert_eq!(
        exfat.data("DVD/SLUS_212.18.Huge Game.iso").unwrap().len(),
        len
    );
    assert_same_as(&exfat, "DVD/SLUS_212.18.Huge Game.iso", &iso, 0);
    assert!(!exfat.exists("ul.cfg"));
}

#[test]
fn bin_cue_becomes_iso_in_every_mode() {
    let iso = Variant::Retail.build().to_vec();
    for mode in RawMode::ALL {
        for pregap in [0, 150] {
            let fs = MemFs::new(FsKind::ExFat);
            let sources = VirtualSources::new();
            sources.add_file(
                src("Game (USA).bin"),
                Content::Bytes(to_bin(&iso, mode, pregap)),
            );
            sources.add_file(
                src("Game (USA).cue"),
                Content::Bytes(cue_sheet("Game (USA).bin", mode, pregap).into_bytes()),
            );
            // A folder input finds the sheet and does not import the BIN twice.
            let plan = plan_import(
                &fs,
                &sources,
                &[std::path::PathBuf::from("src")],
                &options(),
            )
            .unwrap();
            assert!(plan.skipped.is_empty(), "{mode:?}: {:?}", plan.skipped);
            assert_eq!(plan.transactions.len(), 2, "{mode:?}");
            let game = plan.transactions[1].game.clone().unwrap();
            assert_eq!(game.startup, "SLUS_200.62");
            // A raw CD dump is a CD; MODE1/2048 is just an ISO with another name.
            if mode != RawMode::Mode1_2048 {
                assert_eq!(game.media, Media::Cd);
            }
            assert!(run(&plan, &fs, &sources).succeeded(), "{mode:?}");
            assert_eq!(
                fs.bytes("CD/SLUS_200.62.Game (USA).iso").unwrap(),
                iso,
                "{mode:?} pregap {pregap}"
            );
        }
    }
}

#[test]
fn lone_bin_and_broken_sheets() {
    let iso = Variant::Retail.build().to_vec();
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_file(
        src("Lone.bin"),
        Content::Bytes(to_bin(&iso, RawMode::Mode2_2352, 0)),
    );
    sources.add_file(
        src("Multi.cue"),
        Content::Bytes(
            b"FILE \"Multi.bin\" BINARY\n TRACK 01 MODE2/2352\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 01 10:00:00\n".to_vec(),
        ),
    );
    sources.add_file(
        src("Multi.bin"),
        Content::Bytes(to_bin(&iso, RawMode::Mode2_2352, 0)),
    );
    sources.add_file(
        src("Missing.cue"),
        Content::Bytes(cue_sheet("Gone.bin", RawMode::Mode2_2352, 0).into_bytes()),
    );
    let mut truncated = to_bin(&iso, RawMode::Mode2_2352, 0);
    truncated.truncate(truncated.len() - 100);
    sources.add_file(src("Short.bin"), Content::Bytes(truncated));
    sources.add_file(
        src("Short.cue"),
        Content::Bytes(cue_sheet("Short.bin", RawMode::Mode2_2352, 0).into_bytes()),
    );
    sources.add_file(src("Game.chd"), Content::Bytes(vec![0; 10]));

    let plan = plan_import(
        &fs,
        &sources,
        &[std::path::PathBuf::from("src")],
        &options(),
    )
    .unwrap();
    let lone = plan
        .transactions
        .iter()
        .find(|t| t.title.contains("Lone"))
        .unwrap();
    assert!(lone.notices.iter().any(|n| n.code == "binLayoutDetected"));
    let mut codes = skipped_codes(&plan);
    codes.sort();
    // Multi.cue says its data track runs 10 minutes, longer than Multi.bin;
    // Game.chd is no CHD at all. Short.bin belongs to Short.cue: one entry,
    // not two.
    assert_eq!(
        codes,
        [
            "unsupportedSource",
            "unsupportedSource",
            "unsupportedSource",
            "unsupportedSource"
        ],
        "{:?}",
        plan.skipped
    );
    // The BIN of the refused sheet is not imported on its own either.
    assert!(!plan.transactions.iter().any(|t| t.title.contains("Multi")));
    assert!(run(&plan, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Lone.iso").unwrap(), iso);
}

fn zso_of(iso: &[u8]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    zso::compress(
        &mut Cursor::new(iso),
        iso.len() as u64,
        &mut out,
        &mut |_| {},
    )
    .unwrap();
    out.into_inner()
}

#[test]
fn zso_is_kept_or_decompressed_by_profile() {
    let iso = Variant::Retail.build().to_vec();
    let compressed = zso_of(&iso);
    let sources = VirtualSources::new();
    sources.add_file(src("Retail.zso"), Content::Bytes(compressed.clone()));

    let fs = MemFs::new(FsKind::ExFat);
    let p = plan_for(&fs, &sources, &["Retail.zso"], &options());
    assert!(run(&p, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Retail.zso").unwrap(), compressed);

    // OPL 1.1.0 cannot read ZSO: store the ISO inside.
    let fs = MemFs::new(FsKind::Fat32);
    let old = ImportOptions {
        profile: LoaderProfile::Opl110,
        ..options()
    };
    let p = plan_for(&fs, &sources, &["Retail.zso"], &old);
    assert!(
        p.transactions[1]
            .notices
            .iter()
            .any(|n| n.code == "zsoDecompressedForOpl110")
    );
    assert!(run(&p, &fs, &sources).succeeded());
    assert_eq!(fs.bytes("CD/SLUS_200.62.Retail.iso").unwrap(), iso);
}

#[test]
fn what_opl_would_not_show_is_skipped() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_iso(src("No Boot.iso"), Variant::NoBoot2.build());
    sources.add_iso(src("Ps1.iso"), Variant::Ps1.build());
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    fs.insert_file("cd/slus_200.62.retail.ISO", b"older copy");
    let p = plan_for(
        &fs,
        &sources,
        &["No Boot.iso", "Ps1.iso", "Retail.iso", "Nowhere.iso"],
        &options(),
    );
    assert_eq!(
        skipped_codes(&p),
        ["sourceMissing", "noStartup", "ps1Game", "nameTaken"]
    );
    assert!(!p.is_executable());
    assert!(p.notices.iter().any(|n| n.code == "nothingToImport"));
}

#[test]
fn names_follow_the_chosen_style() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    sources.add_iso(
        src("SLUS_200.62.Grand Theft Auto: III?.iso"),
        Variant::Retail.build(),
    );
    sources.add_iso(src("Sub.iso"), Variant::SubfolderElf.build());
    let p = plan_for(
        &fs,
        &sources,
        &["SLUS_200.62.Grand Theft Auto: III?.iso", "Sub.iso"],
        &options(),
    );
    let paths: Vec<String> = p
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter_map(|s| match s {
            Step::WriteFile { path, .. } => Some(path.to_string()),
            _ => None,
        })
        .collect();
    // The ID prefix is not doubled, forbidden characters go, and an ID
    // that cannot lead a name falls back to Title.iso.
    assert_eq!(
        paths,
        ["CD/SLUS_200.62.Grand Theft Auto III.iso", "CD/Sub.iso"]
    );
    let title_only = ImportOptions {
        name_style: NameStyle::TitleOnly,
        ..options()
    };
    let p = plan_for(&fs, &sources, &["Sub.iso"], &title_only);
    assert!(
        p.transactions[1].notices.is_empty()
            || p.transactions[1]
                .notices
                .iter()
                .all(|n| n.level != Level::Error)
    );
}

#[test]
fn bare_ids_take_the_title_from_the_database() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    // Retail is SLUS_200.62: Grand Theft Auto III in the bundled database.
    sources.add_iso(src("SLUS_200.62.iso"), Variant::Retail.build());
    sources.add_iso(src("slus-20062.iso"), Variant::Retail.build());
    sources.add_iso(src("Keep Me.iso"), Variant::Retail.build());
    let p = plan_for(
        &fs,
        &sources,
        &["SLUS_200.62.iso", "slus-20062.iso", "Keep Me.iso"],
        &options(),
    );
    let paths: Vec<String> = p
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter_map(|s| match s {
            Step::WriteFile { path, .. } => Some(path.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(
        paths,
        [
            "CD/SLUS_200.62.Grand Theft Auto III.iso",
            "CD/SLUS_200.62.Keep Me.iso",
        ],
        "{:?}",
        p.skipped
    );
    assert!(
        p.transactions[1]
            .notices
            .iter()
            .any(|n| n.code == "titleFromDatabase"),
        "{:?}",
        p.transactions[1].notices
    );
    assert_eq!(skipped_codes(&p), ["nameTaken"]);
}

#[test]
fn move_deletes_sources_only_after_the_copy() {
    let fs = MemFs::new(FsKind::ExFat);
    let sources = VirtualSources::new();
    let iso = Variant::Retail.build().to_vec();
    sources.add_file(
        src("Game.bin"),
        Content::Bytes(to_bin(&iso, RawMode::Mode2_2352, 0)),
    );
    sources.add_file(
        src("Game.cue"),
        Content::Bytes(cue_sheet("Game.bin", RawMode::Mode2_2352, 0).into_bytes()),
    );
    let moving = ImportOptions {
        transfer: Transfer::Move,
        ..options()
    };
    let p = plan_for(&fs, &sources, &["Game.cue"], &moving);
    assert!(p.deletes_user_data());
    assert!(p.notices.iter().any(|n| n.code == "sourcesDeleted"));
    assert!(run(&p, &fs, &sources).succeeded());
    assert_eq!(sources.removed(), [src("Game.cue"), src("Game.bin")]);
    assert_eq!(fs.bytes("CD/SLUS_200.62.Game.iso").unwrap(), iso);
}

#[test]
fn games_bin_is_removed_where_images_change() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file("CD/games.bin", &[0u8; 188]);
    fs.insert_file("DVD/games.bin", &[0u8; 188]);
    let sources = VirtualSources::new();
    sources.add_iso(src("A.iso"), Variant::Retail.build());
    sources.add_iso(src("B.iso"), Variant::LongId.build());
    let p = plan_for(&fs, &sources, &["A.iso", "B.iso"], &options());
    let removals = p
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter(|s| matches!(s, Step::Remove { .. }))
        .count();
    assert_eq!(removals, 1, "once per changed folder");
    assert!(run(&p, &fs, &sources).succeeded());
    assert!(!fs.exists("CD/games.bin"));
    assert!(fs.exists("DVD/games.bin"));
}

#[test]
fn space_smb_and_device_checks() {
    let sources = VirtualSources::new();
    sources.add_iso(src("Huge.iso"), big_iso("SLUS_212.18", 4_400_000_000));
    let small = MemFs::new(FsKind::ExFat).with_free_bytes(1 << 30);
    let p = plan_for(&small, &sources, &["Huge.iso"], &options());
    assert!(
        p.notices
            .iter()
            .any(|n| n.code == "noSpace" && n.level == Level::Error)
    );
    assert!(!p.is_executable());
    // Running it anyway names what is missing, sizes included (ADR-0029).
    let blocked = run(&p, &small, &sources).error.unwrap();
    let said = blocked.phrase();
    assert_eq!(said.key, "errors.planBlocked");
    assert!(
        blocked.to_string().starts_with(
            "The plan cannot run. Not enough free space: 4.1 GiB needed, 1.0 GiB available"
        ),
        "{blocked}"
    );

    // Over SMB OPL opens at most 10 ul parts.
    sources.add_iso(src("Eleven.iso"), big_iso("SLUS_212.19", 10 * GIB + 4096));
    let share = MemFs::new(FsKind::Ntfs).with_remote(true);
    let ul = ImportOptions {
        layout: Layout::Ul,
        ..options()
    };
    let p = plan_for(&share, &sources, &["Eleven.iso"], &ul);
    assert_eq!(skipped_codes(&p), ["ulTooManyPartsForSmb"]);

    // A swapped stick or a changed source stops the plan before any write.
    let fs = MemFs::new(FsKind::ExFat);
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    let p = plan_for(&fs, &sources, &["Retail.iso"], &options());
    fs.set_fingerprint("OTHER-STICK");
    let report = run(&p, &fs, &sources);
    assert!(report.error.unwrap().to_string().contains("device changed"));
    assert!(fs.files().is_empty());
    fs.set_fingerprint("MEM-0001");
    sources.touch(&src("Retail.iso"));
    let report = run(&p, &fs, &sources);
    assert!(report.error.unwrap().to_string().contains("changed since"));
    assert!(fs.files().is_empty());
}

#[test]
fn ul_cfg_keeps_every_other_byte() {
    let fs = MemFs::new(FsKind::Fat32);
    let mut existing = yaoplm_formats::ul_cfg::UlRecord::new("Old", "SLES_123.45", 1, Media::Cd)
        .unwrap()
        .as_bytes()
        .to_vec();
    existing[0x3A] = 0x55;
    existing.extend_from_slice(b"ragged tail");
    fs.insert_file("ul.cfg", &existing);
    let sources = VirtualSources::new();
    sources.add_iso(src("New.iso"), Variant::Retail.build());
    let forced = ImportOptions {
        layout: Layout::Ul,
        ..options()
    };
    let p = plan_for(&fs, &sources, &["New.iso"], &forced);
    assert!(run(&p, &fs, &sources).succeeded());
    let after = fs.bytes("ul.cfg").unwrap();
    assert_eq!(&after[..64], &existing[..64]);
    assert_eq!(UlCfg::parse(&after[64..128]).records[0].name(), "New");
    assert_eq!(&after[128..], b"ragged tail");
}

#[test]
fn an_unfinished_journal_blocks_new_plans() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file(".yaoplm/journal/20261022-000000-000001.json", b"{not json");
    let sources = VirtualSources::new();
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    let p = plan_for(&fs, &sources, &["Retail.iso"], &options());
    assert!(p.notices.iter().any(|n| n.code == "unfinishedOperation"));
    assert!(!p.is_executable());
}
