//! Property tests for the engine: wherever the device vanishes, recovery
//! leaves a consistent library (ADR-0021), and planned names always fit
//! OPL's limits (IMP-4, IMP-5).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{files, run, src};
use proptest::prelude::*;
use yaoplm_core::import::{ImportOptions, Layout, plan_import, title_from_path, ul_title};
use yaoplm_core::plan::Step;
use yaoplm_core::recovery::{RecoveryAction, plan_recovery};
use yaoplm_device::FsKind;
use yaoplm_formats::Media;
use yaoplm_formats::iso_name::{self, NameFormat};
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

const GAMES: [(&str, Variant); 3] = [
    ("A.iso", Variant::Retail),
    ("B.iso", Variant::LfOnly),
    ("C.iso", Variant::LongId),
];

/// A FAT32 stick with one ul game already on it, and three to import.
fn setup() -> (MemFs, VirtualSources, Vec<u8>) {
    let fs = MemFs::new(FsKind::Fat32);
    let existing = UlRecord::new("Old", "SLES_123.45", 1, Media::Cd).unwrap();
    fs.insert_file("ul.cfg", existing.as_bytes());
    fs.insert_file(&existing.chunk_name(0), b"old chunk");
    fs.insert_file("CD/games.bin", &[0u8; 188]);
    let sources = VirtualSources::new();
    for (name, variant) in GAMES {
        sources.add_iso(src(name), variant.build());
    }
    (fs, sources, existing.as_bytes().to_vec())
}

/// Every ul.cfg record has all its chunks, every chunk belongs to a record,
/// nothing half written is left. Returns the record count.
fn assert_consistent(fs: &MemFs) -> usize {
    let table = UlCfg::parse(&fs.bytes("ul.cfg").unwrap_or_default());
    assert!(table.trailing.is_empty());
    let mut expected: Vec<String> = table
        .records
        .iter()
        .flat_map(UlRecord::chunk_names)
        .map(|n| n.to_lowercase())
        .collect();
    expected.sort();
    let mut chunks: Vec<String> = files(fs)
        .into_iter()
        .map(|(p, _)| p.to_lowercase())
        .filter(|p| p.starts_with("ul.") && p != "ul.cfg")
        .collect();
    chunks.sort();
    assert_eq!(chunks, expected, "{:?}", fs.files());
    assert!(
        fs.files()
            .iter()
            .all(|(p, _)| !p.contains(".yaoplm-part") && !p.contains(".yaoplm-tmp")),
        "{:?}",
        fs.files()
    );
    assert!(
        fs.files()
            .iter()
            .all(|(p, _)| !p.starts_with(".yaoplm/journal/")),
        "journal left: {:?}",
        fs.files()
    );
    table.records.len()
}

fn import_plan(fs: &MemFs, sources: &VirtualSources) -> yaoplm_core::plan::Plan {
    let inputs: Vec<_> = GAMES.iter().map(|(name, _)| src(name)).collect();
    let options = ImportOptions {
        layout: Layout::Ul,
        ..ImportOptions::default()
    };
    plan_import(fs, sources, &inputs, &options).unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Unplug after any number of bytes, plug back in, recover: rollback
    /// keeps the old game and only fully imported ones; resume ends with
    /// all of them.
    #[test]
    fn recovery_after_any_interruption(cut in 0u64..400_000, resume in any::<bool>()) {
        let (fs, sources, before) = setup();
        let plan = import_plan(&fs, &sources);
        fs.unplug_after(cut);
        let first = run(&plan, &fs, &sources);
        fs.replug();
        if first.journal.is_some() {
            let action = if resume { RecoveryAction::Resume } else { RecoveryAction::Rollback };
            let recovery = plan_recovery(&fs, action).unwrap();
            let report = run(&recovery, &fs, &sources);
            prop_assert!(report.succeeded(), "{:?}", report.error);
        }
        // The next operation cleans up what a write cut short left behind.
        let next = yaoplm_core::ops::plan_init(&fs).unwrap();
        prop_assert!(run(&next, &fs, &sources).succeeded());
        let records = assert_consistent(&fs);
        let after = fs.bytes("ul.cfg").unwrap();
        // The game that was there keeps every byte, in first place.
        prop_assert_eq!(&after[..64], &before[..]);
        match (first.succeeded(), &first.journal) {
            (true, _) => prop_assert_eq!(records, 4),
            // Cut before the journal existed: nothing was written.
            (false, None) => prop_assert!((1..=4).contains(&records)),
            (false, Some(_)) if resume => prop_assert_eq!(records, 4),
            (false, Some(_)) => prop_assert!((1..=4).contains(&records)),
        }
    }

    /// Titles become names OPL lists back with the same ID, within 160
    /// characters and 159 bytes; ul titles stay printable ASCII <= 32.
    #[test]
    fn planned_names_fit_opl(title in "\\PC{0,200}") {
        let ul = ul_title(&title);
        prop_assert!(ul.len() <= 32);
        prop_assert!(ul.bytes().all(|b| (0x20..0x7F).contains(&b)));
        prop_assert!(!ul.ends_with(' ') && !ul.starts_with(' '));
        if !ul.is_empty() {
            prop_assert!(UlRecord::new(&ul, "SLUS_200.62", 1, Media::Cd).is_ok());
        }

        let fs = MemFs::new(FsKind::ExFat);
        let sources = VirtualSources::new();
        sources.add_iso(src("game.iso"), Variant::Retail.build());
        let options = ImportOptions {
            overrides: vec![yaoplm_core::import::ImportOverride {
                path: src("game.iso"),
                title: Some(title.clone()),
                media: None,
            }],
            ..ImportOptions::default()
        };
        let plan = plan_import(&fs, &sources, &[src("game.iso")], &options).unwrap();
        let written: Vec<String> = plan
            .transactions
            .iter()
            .flat_map(|t| &t.steps)
            .filter_map(|s| match s {
                Step::WriteFile { path, .. } => Some(path.file_name().to_owned()),
                _ => None,
            })
            .collect();
        prop_assert_eq!(written.len(), 1);
        let name = &written[0];
        prop_assert!(iso_name::fits_smb(name), "{name}");
        let parsed = iso_name::classify(name).unwrap();
        prop_assert_eq!(parsed.format, NameFormat::Old);
        prop_assert_eq!(parsed.startup.as_deref(), Some("SLUS_200.62"));
        prop_assert!(parsed.title.chars().count() <= iso_name::TITLE_MAX);
        prop_assert_eq!(title_from_path(std::path::Path::new(name)), parsed.title);
    }
}
