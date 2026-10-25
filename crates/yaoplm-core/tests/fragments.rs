//! Fragment checks (IMP-7b, LIB-4): after a write and in the library, by
//! the loader's rule, on BDM volumes only. Counts are faked through
//! `MemFs::set_fragments`; the OS side is `yaoplm-device`'s business.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::PathBuf;

use common::{big_iso, run, src};
use yaoplm_core::fragments::check_library;
use yaoplm_core::import::{ImportOptions, plan_import};
use yaoplm_core::plan::{Plan, Step};
use yaoplm_core::{
    CfgSummary, Fragments, Game, GameFormat, GameLocation, Library, LoaderProfile, Media, UlPart,
    Warning,
};
use yaoplm_device::FsKind;
use yaoplm_formats::cfg::Dialect;
use yaoplm_formats::iso_name::NameFormat;
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

const RETAIL: &str = "CD/SLUS_200.62.Retail.iso";

fn import(fs: &MemFs, profile: LoaderProfile) -> (Plan, VirtualSources) {
    let sources = VirtualSources::new();
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    sources.add_iso(src("Huge Game.iso"), big_iso("SLUS_212.18", 4_400_000_000));
    let inputs = [src("Retail.iso"), src("Huge Game.iso")];
    let options = ImportOptions {
        profile,
        ..ImportOptions::default()
    };
    (
        plan_import(fs, &sources, &inputs, &options).unwrap(),
        sources,
    )
}

/// The files the plan writes for the game with `startup`.
fn written(plan: &Plan, startup: &str) -> Vec<String> {
    let tx = plan
        .transactions
        .iter()
        .find(|tx| tx.game.as_ref().is_some_and(|g| g.startup == startup))
        .unwrap();
    tx.steps
        .iter()
        .filter_map(|step| match step {
            Step::WriteFile { path, .. } | Step::WriteZso { path, .. } => {
                Some(path.as_str().to_owned())
            }
            _ => None,
        })
        .collect()
}

fn codes(report: &yaoplm_core::exec::Report) -> Vec<(&str, String)> {
    report
        .notices
        .iter()
        .map(|(_, n)| (n.code.as_str(), n.message.clone()))
        .collect()
}

#[test]
fn a_game_over_the_limit_in_total_is_reported_after_the_write() {
    let fs = MemFs::new(FsKind::Fat32);
    let (plan, sources) = import(&fs, LoaderProfile::Opl120Beta);
    assert_eq!(plan.profile, Some(LoaderProfile::Opl120Beta));
    let chunks = written(&plan, "SLUS_212.18");
    assert_eq!(chunks.len(), 5, "{chunks:?}");
    // Every chunk fits OPL's table alone; together they do not.
    for chunk in &chunks {
        fs.set_fragments(chunk, 13);
    }
    fs.set_fragments(RETAIL, 64);
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);
    let found = codes(&report);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "gameFragmented");
    assert!(found[0].1.contains("65"), "{}", found[0].1);
    assert!(found[0].1.contains("64"), "{}", found[0].1);
    // Reported for the transaction of that game.
    let index = report.notices[0].0;
    assert_eq!(
        plan.transactions[index].game.as_ref().unwrap().startup,
        "SLUS_212.18"
    );
}

#[test]
fn opl_110_wants_every_file_in_one_piece() {
    let fs = MemFs::new(FsKind::Fat32);
    let (plan, sources) = import(&fs, LoaderProfile::Opl110);
    fs.set_fragments(RETAIL, 2);
    for chunk in written(&plan, "SLUS_212.18") {
        fs.set_fragments(&chunk, 1);
    }
    let report = run(&plan, &fs, &sources);
    let found = codes(&report);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "gameFragmentedOpl110");
}

#[test]
fn no_check_off_bdm_or_when_the_os_cannot_tell() {
    for fs in [
        MemFs::new(FsKind::Ntfs),
        MemFs::new(FsKind::Fat32).with_smb(true),
    ] {
        let (plan, sources) = import(&fs, LoaderProfile::Opl120Beta);
        fs.set_fragments(RETAIL, 500);
        let report = run(&plan, &fs, &sources);
        assert!(report.succeeded(), "{:?}", report.error);
        assert!(report.notices.is_empty(), "{:?}", report.notices);
    }
    // No count for one chunk: the game is unknown, not "fine" or "bad".
    let fs = MemFs::new(FsKind::Fat32);
    let (plan, sources) = import(&fs, LoaderProfile::Opl120Beta);
    let chunks = written(&plan, "SLUS_212.18");
    for chunk in &chunks[1..] {
        fs.set_fragments(chunk, 60);
    }
    let report = run(&plan, &fs, &sources);
    assert!(report.notices.is_empty(), "{:?}", report.notices);
}

#[test]
fn the_profile_travels_with_the_plan() {
    let fs = MemFs::new(FsKind::Fat32);
    let (plan, _) = import(&fs, LoaderProfile::Opl110);
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains(r#""profile":"opl-1.1""#), "{json}");
    let back: Plan = serde_json::from_str(&json).unwrap();
    assert_eq!(back.profile, Some(LoaderProfile::Opl110));
    // Older journals have none; a name from a newer build reads as none.
    let old = json.replace(r#""profile":"opl-1.1""#, r#""profile":"nhddl""#);
    assert_eq!(serde_json::from_str::<Plan>(&old).unwrap().profile, None);
}

fn game(location: GameLocation, vmc: [Option<String>; 2]) -> Game {
    Game {
        title: "Game".into(),
        startup: "SLUS_200.62".into(),
        media: Media::Dvd,
        format: GameFormat::Iso,
        location,
        size: 0,
        image_startup: None,
        layers: None,
        cfg: Some(CfgSummary {
            path: PathBuf::from("/lib/CFG/SLUS_200.62.cfg"),
            dialect: Dialect::Opl,
            compatibility: None,
            vmc,
        }),
        art: Vec::new(),
        cheats: None,
        fragments: None,
        warnings: Vec::new(),
    }
}

fn library(games: Vec<Game>) -> Library {
    Library {
        root: PathBuf::from("/lib"),
        folders: Vec::new(),
        games,
        apps: Vec::new(),
        skipped: Vec::new(),
        warnings: Vec::new(),
    }
}

fn image(path: &str) -> GameLocation {
    GameLocation::Image {
        path: PathBuf::from("/lib").join(path),
        file_name: path.rsplit('/').next().unwrap().into(),
        name_format: NameFormat::New,
    }
}

#[test]
fn the_library_check_counts_games_and_their_cards() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_file("DVD/Game.iso", b"x");
    fs.insert_file("DVD/Other.iso", b"x");
    fs.insert_file("VMC/Card.bin", b"x");
    fs.set_fragments("DVD/Game.iso", 70);
    fs.set_fragments("DVD/Other.iso", 3);
    fs.set_fragments("VMC/Card.bin", 2);
    let parts = (0..2)
        .map(|n| UlPart {
            name: format!("ul.ABCD1234.SLUS_212.18.0{n}"),
            path: Some(PathBuf::from(format!("/lib/ul.ABCD1234.SLUS_212.18.0{n}"))),
            size: Some(1),
        })
        .collect();
    fs.insert_file("ul.ABCD1234.SLUS_212.18.00", b"x");
    fs.insert_file("ul.ABCD1234.SLUS_212.18.01", b"x");
    fs.set_fragments("ul.ABCD1234.SLUS_212.18.00", 1);
    fs.set_fragments("ul.ABCD1234.SLUS_212.18.01", 1);
    let mut lib = library(vec![
        game(image("DVD/Game.iso"), [None, None]),
        // The card file is matched the way FAT does: ignoring case.
        game(image("DVD/Other.iso"), [None, Some("card".into())]),
        game(GameLocation::Ul { index: 0, parts }, [None, None]),
    ]);
    check_library(&fs, &mut lib, LoaderProfile::Opl120Beta, &|| false);

    assert_eq!(lib.games[0].fragments, Some(Fragments::Known(vec![70])));
    assert_eq!(
        lib.games[0].warnings,
        [Warning::Fragmented {
            fragments: 70,
            limit: 64
        }]
    );
    assert_eq!(lib.games[1].fragments, Some(Fragments::Known(vec![3])));
    assert_eq!(
        lib.games[1].warnings,
        [Warning::VmcFragmented {
            slot: 1,
            name: "card".into(),
            fragments: 2
        }]
    );
    assert_eq!(lib.games[2].fragments, Some(Fragments::Known(vec![1, 1])));
    assert!(lib.games[2].warnings.is_empty());
    assert!(
        lib.games[1].warnings[0].to_string().contains("slot 2"),
        "{}",
        lib.games[1].warnings[0]
    );
}

#[test]
fn the_library_check_knows_when_it_cannot_tell() {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file("CD/Game.iso", b"x");
    let mut lib = library(vec![game(image("CD/Game.iso"), [None, None])]);
    check_library(&fs, &mut lib, LoaderProfile::Opl120Beta, &|| false);
    assert_eq!(lib.games[0].fragments, Some(Fragments::Unknown));
    assert!(lib.games[0].warnings.is_empty());

    // Off BDM nothing is counted at all.
    let fs = MemFs::new(FsKind::Ntfs);
    fs.insert_file("CD/Game.iso", b"x");
    fs.set_fragments("CD/Game.iso", 100);
    let mut lib = library(vec![game(image("CD/Game.iso"), [None, None])]);
    check_library(&fs, &mut lib, LoaderProfile::Opl120Beta, &|| false);
    assert_eq!(lib.games[0].fragments, None);

    // A cancelled check stops before the first game.
    let fs = MemFs::new(FsKind::Fat32);
    let mut lib = library(vec![game(image("CD/Game.iso"), [None, None])]);
    check_library(&fs, &mut lib, LoaderProfile::Opl120Beta, &|| true);
    assert_eq!(lib.games[0].fragments, None);
}
