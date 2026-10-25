//! Rename, delete and move games already on the device (GAME-1 to GAME-3,
//! CFG-5, LIB-3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{files, run};
use yaoplm_core::Media;
use yaoplm_core::exec::Outcome;
use yaoplm_core::ops::{
    DeleteOptions, GameRef, OpError, RenameOptions, plan_delete, plan_init, plan_init_folder,
    plan_move, plan_rename,
};
use yaoplm_core::plan::Step;
use yaoplm_device::{FsKind, RelPath};
use yaoplm_formats::iso_name::NameStyle;
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_test_support::{MemFs, Variant, VirtualSources};

fn image(path: &str) -> GameRef {
    GameRef::Image {
        path: RelPath::new(path).unwrap(),
    }
}

fn rename(title: &str) -> RenameOptions {
    RenameOptions {
        title: title.into(),
        ..RenameOptions::default()
    }
}

/// A stick with one image, its CFG, art, cheats and a VMC.
fn library() -> MemFs {
    let fs = MemFs::new(FsKind::Fat32);
    fs.insert_file(
        "CD/SLUS_200.62.Retail.iso",
        &Variant::Retail.build().to_vec(),
    );
    fs.insert_file("CD/games.bin", &[0u8; 188]);
    fs.insert_file(
        "CFG/SLUS_200.62.cfg",
        b"$Compatibility=3\r\n$VMC_0=SLUS_200.62_0\r\nUnknown=kept\r\n",
    );
    fs.insert_file("ART/SLUS_200.62_COV.png", b"png");
    fs.insert_file("ART/SLUS_200.62_ICO.png", b"png");
    fs.insert_file("CHT/SLUS_200.62.cht", b"Mastercode\n");
    fs.insert_file("VMC/SLUS_200.62_0.bin", b"card");
    fs
}

fn paths(fs: &MemFs) -> Vec<String> {
    files(fs).into_iter().map(|(p, _)| p).collect()
}

fn backups(fs: &MemFs) -> Vec<String> {
    fs.files()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p.starts_with(".yaoplm/backup/"))
        .collect()
}

#[test]
fn a_new_title_renames_only_the_image() {
    let fs = library();
    let src = VirtualSources::new();
    let plan = plan_rename(
        &fs,
        &image("CD/SLUS_200.62.Retail.iso"),
        &rename("Grand: Theft Auto"),
    )
    .unwrap();
    assert!(run(&plan, &fs, &src).succeeded());
    let names = paths(&fs);
    assert!(
        names.contains(&"CD/SLUS_200.62.Grand Theft Auto.iso".to_owned()),
        "{names:?}"
    );
    assert!(!names.contains(&"CD/games.bin".to_owned()));
    for kept in [
        "CFG/SLUS_200.62.cfg",
        "ART/SLUS_200.62_COV.png",
        "CHT/SLUS_200.62.cht",
        "VMC/SLUS_200.62_0.bin",
    ] {
        assert!(names.contains(&kept.to_owned()), "{kept}");
    }
    assert!(backups(&fs).is_empty(), "no CFG was touched");

    // Same name: nothing to do. Existing name: refused before any write.
    let err = plan_rename(
        &fs,
        &image("CD/SLUS_200.62.Grand Theft Auto.iso"),
        &rename("Grand Theft Auto"),
    )
    .unwrap_err();
    assert!(matches!(err, OpError::Invalid(_)));
    fs.insert_file("CD/SLUS_200.62.Taken.iso", b"x");
    let err = plan_rename(
        &fs,
        &image("CD/SLUS_200.62.Grand Theft Auto.iso"),
        &rename("Taken"),
    )
    .unwrap_err();
    assert!(matches!(err, OpError::Conflict(_)));
}

#[test]
fn case_only_renames_go_through_a_temporary_name() {
    let fs = library();
    let plan = plan_rename(&fs, &image("CD/SLUS_200.62.Retail.iso"), &rename("RETAIL")).unwrap();
    let renames = plan.transactions[0]
        .steps
        .iter()
        .filter(|s| matches!(s, Step::Rename { .. }))
        .count();
    assert_eq!(renames, 2);
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert!(paths(&fs).contains(&"CD/SLUS_200.62.RETAIL.iso".to_owned()));
}

#[test]
fn a_new_id_moves_cfg_art_and_cheats_but_not_the_vmc() {
    let fs = library();
    let options = RenameOptions {
        title: "Retail".into(),
        startup: Some("SLES_500.51".into()),
        name_style: None,
    };
    let plan = plan_rename(&fs, &image("CD/SLUS_200.62.Retail.iso"), &options).unwrap();
    assert!(
        plan.transactions[0]
            .notices
            .iter()
            .any(|n| n.code == "relatedMoved")
    );
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let names = paths(&fs);
    for moved in [
        "CD/SLES_500.51.Retail.iso",
        "CFG/SLES_500.51.cfg",
        "ART/SLES_500.51_COV.png",
        "ART/SLES_500.51_ICO.png",
        "CHT/SLES_500.51.cht",
        "VMC/SLUS_200.62_0.bin",
    ] {
        assert!(names.contains(&moved.to_owned()), "{moved}: {names:?}");
    }
    // The CFG still points at the same card, unknown keys and all.
    assert_eq!(
        fs.bytes("CFG/SLES_500.51.cfg").unwrap(),
        b"$Compatibility=3\r\n$VMC_0=SLUS_200.62_0\r\nUnknown=kept\r\n"
    );
    let backups = backups(&fs);
    assert_eq!(backups.len(), 1);
    assert!(backups[0].ends_with("/CFG/SLUS_200.62.cfg"), "{backups:?}");
}

#[test]
fn a_shared_id_copies_instead_of_moving() {
    let fs = library();
    fs.insert_file(
        "DVD/SLUS_200.62.Other Disc.iso",
        &Variant::Retail.build().to_vec(),
    );
    let options = RenameOptions {
        title: "Retail".into(),
        startup: Some("SLES_500.51".into()),
        name_style: None,
    };
    let plan = plan_rename(&fs, &image("CD/SLUS_200.62.Retail.iso"), &options).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let names = paths(&fs);
    assert!(names.contains(&"CFG/SLUS_200.62.cfg".to_owned()));
    assert!(names.contains(&"CFG/SLES_500.51.cfg".to_owned()));
}

#[test]
fn switching_name_style_reads_the_id_from_the_image() {
    let fs = library();
    let options = RenameOptions {
        title: "Retail".into(),
        startup: None,
        name_style: Some(NameStyle::TitleOnly),
    };
    let plan = plan_rename(&fs, &image("CD/SLUS_200.62.Retail.iso"), &options).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert!(paths(&fs).contains(&"CD/Retail.iso".to_owned()));
    // The image says SLUS_200.62 too, so CFG and art stay.
    assert!(paths(&fs).contains(&"CFG/SLUS_200.62.cfg".to_owned()));
}

fn ul_library() -> (MemFs, Vec<u8>) {
    let fs = MemFs::new(FsKind::Fat32);
    let a = UlRecord::new("First Game", "SLUS_111.11", 2, Media::Dvd).unwrap();
    let mut b_bytes = *UlRecord::new("Second", "SLES_222.22", 1, Media::Cd)
        .unwrap()
        .as_bytes();
    b_bytes[0x3C] = 0x42;
    let b = UlRecord::from_bytes(b_bytes);
    let mut cfg = a.as_bytes().to_vec();
    cfg.extend_from_slice(b.as_bytes());
    fs.insert_file("ul.cfg", &cfg);
    for name in a.chunk_names().iter().chain(&b.chunk_names()) {
        fs.insert_file(name, b"chunk");
    }
    (fs, cfg)
}

fn ul_ref(index: usize, startup: &str, name: &str) -> GameRef {
    GameRef::Ul {
        index,
        startup: startup.into(),
        name: name.into(),
    }
}

#[test]
fn renaming_a_ul_game_renames_its_chunks() {
    let (fs, before) = ul_library();
    let plan = plan_rename(
        &fs,
        &ul_ref(0, "SLUS_111.11", "First Game"),
        &rename("First: Game II"),
    )
    .unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let table = UlCfg::parse(&fs.bytes("ul.cfg").unwrap());
    assert_eq!(table.records[0].name(), "First: Game II");
    // The other record keeps every byte.
    assert_eq!(&fs.bytes("ul.cfg").unwrap()[64..], &before[64..]);
    let expected: Vec<String> = table.records[0].chunk_names();
    for chunk in &expected {
        assert!(fs.exists(chunk), "{chunk}");
    }
    assert!(!fs.exists(&UlRecord::from_bytes(before[..64].try_into().unwrap()).chunk_name(0)));

    // A stale reference never hits another game.
    let err = plan_rename(&fs, &ul_ref(0, "SLUS_111.11", "First Game"), &rename("X")).unwrap_err();
    assert!(matches!(err, OpError::NotFound(_)));
    // ul titles are ASCII and at most 32 bytes.
    assert!(
        plan_rename(
            &fs,
            &ul_ref(0, "SLUS_111.11", "First: Game II"),
            &rename("Ōkami")
        )
        .is_err()
    );
    assert!(
        plan_rename(
            &fs,
            &ul_ref(0, "SLUS_111.11", "First: Game II"),
            &rename(&"A".repeat(33))
        )
        .is_err()
    );
}

#[test]
fn a_failed_chunk_rename_is_undone() {
    let (fs, before) = ul_library();
    let old = UlRecord::from_bytes(before[..64].try_into().unwrap());
    let plan = plan_rename(
        &fs,
        &ul_ref(0, "SLUS_111.11", "First Game"),
        &rename("Renamed"),
    )
    .unwrap();
    let new = UlRecord::from_bytes(before[..64].try_into().unwrap())
        .renamed("Renamed")
        .unwrap();
    fs.fail_once("rename", &old.chunk_name(1));
    let report = run(&plan, &fs, &VirtualSources::new());
    assert_eq!(report.outcomes, [Outcome::RolledBack]);
    assert_eq!(fs.bytes("ul.cfg").unwrap(), before);
    assert!(fs.exists(&old.chunk_name(0)) && fs.exists(&old.chunk_name(1)));
    assert!(!fs.exists(&new.chunk_name(0)));
}

#[test]
fn deleting_keeps_settings_unless_asked() {
    let fs = library();
    let src = VirtualSources::new();
    let plan = plan_delete(
        &fs,
        &image("CD/SLUS_200.62.Retail.iso"),
        &DeleteOptions::default(),
    )
    .unwrap();
    assert!(plan.deletes_user_data());
    assert!(run(&plan, &fs, &src).succeeded());
    let names = paths(&fs);
    assert!(!names.iter().any(|n| n.starts_with("CD/")), "{names:?}");
    assert!(names.contains(&"CFG/SLUS_200.62.cfg".to_owned()));

    let fs = library();
    let all = DeleteOptions { related: true };
    let plan = plan_delete(&fs, &image("CD/SLUS_200.62.Retail.iso"), &all).unwrap();
    assert!(run(&plan, &fs, &src).succeeded());
    assert_eq!(
        paths(&fs),
        ["VMC/SLUS_200.62_0.bin"],
        "VMC cards are never deleted"
    );
    assert_eq!(backups(&fs).len(), 1, "the CFG was backed up first");
    let backup = &backups(&fs)[0];
    assert_eq!(
        fs.bytes(backup).unwrap(),
        b"$Compatibility=3\r\n$VMC_0=SLUS_200.62_0\r\nUnknown=kept\r\n"
    );

    // Another game with the same ID keeps CFG, art and cheats.
    let fs = library();
    fs.insert_file("DVD/SLUS_200.62.Disc 2.iso", b"x");
    let plan = plan_delete(&fs, &image("CD/SLUS_200.62.Retail.iso"), &all).unwrap();
    assert!(
        plan.transactions[0]
            .notices
            .iter()
            .any(|n| n.code == "relatedShared")
    );
    assert!(run(&plan, &fs, &src).succeeded());
    assert!(paths(&fs).contains(&"CFG/SLUS_200.62.cfg".to_owned()));
}

#[test]
fn deleting_a_ul_game_drops_its_record_first() {
    let (fs, before) = ul_library();
    let plan = plan_delete(
        &fs,
        &ul_ref(0, "SLUS_111.11", "First Game"),
        &DeleteOptions::default(),
    )
    .unwrap();
    let steps = &plan.transactions[0].steps;
    assert!(matches!(steps[0], Step::PutFile { .. }));
    assert_eq!(plan.transactions[0].commit, 1);
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert_eq!(fs.bytes("ul.cfg").unwrap(), &before[64..]);
    let chunks: Vec<String> = paths(&fs)
        .into_iter()
        .filter(|n| n.starts_with("ul.") && n != "ul.cfg")
        .collect();
    assert_eq!(chunks.len(), 1, "{chunks:?}");
}

#[test]
fn moving_between_cd_and_dvd() {
    let fs = library();
    fs.insert_file("DVD/games.bin", &[0u8; 188]);
    let plan = plan_move(&fs, &image("CD/SLUS_200.62.Retail.iso"), Media::Dvd).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let names = paths(&fs);
    assert!(names.contains(&"DVD/SLUS_200.62.Retail.iso".to_owned()));
    assert!(!names.iter().any(|n| n.ends_with("games.bin")), "{names:?}");
    assert!(plan_move(&fs, &image("DVD/SLUS_200.62.Retail.iso"), Media::Dvd).is_err());

    let (fs, before) = ul_library();
    let plan = plan_move(&fs, &ul_ref(1, "SLES_222.22", "Second"), Media::Dvd).unwrap();
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    let after = fs.bytes("ul.cfg").unwrap();
    assert_eq!(after[64 + 0x30], 0x14);
    assert_eq!(after[64 + 0x3C], 0x42, "other bytes kept");
    assert_eq!(&after[..64], &before[..64]);
}

#[test]
fn init_creates_the_missing_folders() {
    let fs = MemFs::new(FsKind::ExFat);
    fs.insert_dir("cd");
    let plan = plan_init(&fs).unwrap();
    assert_eq!(
        plan.transactions[0].steps.len(),
        8,
        "cd exists, case-insensitively"
    );
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    assert!(plan_init(&fs).unwrap().transactions.is_empty());
}

#[test]
fn init_folder_makes_a_library_inside_the_root() {
    // A whole system disk shared over SMB gets a folder of its own (ADR-0047).
    let fs = MemFs::new(FsKind::Ntfs);
    fs.insert_file("Windows/explorer.exe", b"x");
    let plan = plan_init_folder(&fs, " PS2 ").unwrap();
    let steps = &plan.transactions[0].steps;
    assert_eq!(steps.len(), 10, "the folder and the nine OPL folders");
    assert!(run(&plan, &fs, &VirtualSources::new()).succeeded());
    for name in ["PS2/CD", "PS2/DVD", "PS2/APPS", "PS2/VMC"] {
        assert!(fs.exists(name), "{name}");
    }
    assert!(!fs.exists("CD"), "nothing at the root itself");
    assert!(
        plan_init_folder(&fs, "PS2")
            .unwrap()
            .transactions
            .is_empty()
    );

    // An existing folder only gets what it lacks; a file or a path is refused.
    fs.insert_file("Games/dvd/x.iso", b"x");
    assert_eq!(
        plan_init_folder(&fs, "Games").unwrap().transactions[0]
            .steps
            .len(),
        8
    );
    assert!(matches!(
        plan_init_folder(&fs, "Windows/explorer.exe"),
        Err(OpError::Invalid(_))
    ));
    fs.insert_file("notes.txt", b"x");
    assert!(matches!(
        plan_init_folder(&fs, "notes.txt"),
        Err(OpError::Conflict(_))
    ));
    assert!(matches!(
        plan_init_folder(&fs, ".."),
        Err(OpError::Invalid(_))
    ));
}
