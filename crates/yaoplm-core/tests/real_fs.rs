//! Imports onto real FAT32 and exFAT drivers: image files loop-mounted by
//! CI (`.github/workflows/ci.yml`, job `real-fs`), never real drives
//! (CLAUDE.md rule 1). The game is a generated 4.3 GB image, so FAT32 gets
//! `ul` chunks and exFAT one file, both checked byte for byte.
//!
//! Run by hand on Linux after mounting images:
//! `YAOPLM_FAT32_DIR=/mnt/fat32 YAOPLM_EXFAT_DIR=/mnt/exfat cargo test -p yaoplm-core --test real_fs -- --ignored`

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use common::{GIB, big_iso, run, src};
use yaoplm_core::import::{ImportOptions, plan_import};
use yaoplm_core::ops::{DeleteOptions, GameRef, RenameOptions, plan_delete, plan_rename};
use yaoplm_core::{GameFormat, ScanOptions, scan_library};
use yaoplm_device::fs::DeviceFs;
use yaoplm_device::{FsKind, HostFs};
use yaoplm_formats::ul_cfg::CHUNK_SIZE;
use yaoplm_test_support::{Variant, VirtualIso, VirtualSources};

/// Compares a file on the device with the source image from `offset`.
fn assert_file_matches(path: &Path, iso: &VirtualIso, offset: u64) {
    let mut file = File::open(path).unwrap();
    let mut iso = iso.clone();
    iso.seek(SeekFrom::Start(offset)).unwrap();
    let (mut a, mut b) = (vec![0u8; 4 << 20], vec![0u8; 4 << 20]);
    let mut at = 0u64;
    loop {
        let n = file.read(&mut a).unwrap();
        if n == 0 {
            break;
        }
        iso.read_exact(&mut b[..n]).unwrap();
        assert!(
            a[..n] == b[..n],
            "{} differs near byte {at}",
            path.display()
        );
        at += n as u64;
    }
}

fn check(dir: &str, kind: FsKind) {
    let fs = HostFs::new(Path::new(dir)).unwrap();
    assert_eq!(fs.info().unwrap().kind, kind, "{dir} is not {kind}");
    let iso = big_iso("SLUS_212.18", 4_300_000_000);
    let sources = VirtualSources::new();
    sources.add_iso(src("Huge Game.iso"), iso.clone());
    sources.add_iso(src("Retail.iso"), Variant::Retail.build());
    let inputs = [src("Huge Game.iso"), src("Retail.iso")];
    let plan = plan_import(&fs, &sources, &inputs, &ImportOptions::default()).unwrap();
    assert!(plan.is_executable(), "{:?}", plan.notices);
    let report = run(&plan, &fs, &sources);
    assert!(report.succeeded(), "{:?}", report.error);

    let root = fs.root();
    let library = scan_library(root, &ScanOptions::default()).unwrap();
    assert_eq!(library.games.len(), 2, "{library:?}");
    for game in &library.games {
        assert!(
            game.warnings.is_empty(),
            "{}: {:?}",
            game.title,
            game.warnings
        );
    }
    let huge = library
        .games
        .iter()
        .find(|g| g.startup == "SLUS_212.18")
        .unwrap();
    if kind == FsKind::Fat32 {
        assert_eq!(huge.format, GameFormat::Ul);
        let yaoplm_core::GameLocation::Ul { parts, .. } = &huge.location else {
            panic!("expected ul");
        };
        assert_eq!(parts.len(), 5);
        for (n, part) in parts.iter().enumerate() {
            let path = part.path.as_ref().unwrap();
            assert_file_matches(path, &iso, n as u64 * CHUNK_SIZE);
        }
        // Rename and delete on the real driver too.
        let rename = plan_rename(
            &fs,
            &GameRef::Ul {
                index: 0,
                startup: "SLUS_212.18".into(),
                name: "Huge Game".into(),
            },
            &RenameOptions {
                title: "Huge Game Renamed".into(),
                ..RenameOptions::default()
            },
        )
        .unwrap();
        assert!(run(&rename, &fs, &sources).succeeded());
        let delete = plan_delete(
            &fs,
            &GameRef::Ul {
                index: 0,
                startup: "SLUS_212.18".into(),
                name: "Huge Game Renamed".into(),
            },
            &DeleteOptions::default(),
        )
        .unwrap();
        assert!(run(&delete, &fs, &sources).succeeded());
        let after = scan_library(root, &ScanOptions::default()).unwrap();
        assert_eq!(after.games.len(), 1);
    } else {
        assert_eq!(huge.format, GameFormat::Iso);
        assert_eq!(huge.size, iso.len());
        assert!(iso.len() > 4 * GIB);
        let yaoplm_core::GameLocation::Image { path, .. } = &huge.location else {
            panic!("expected an image");
        };
        assert_file_matches(path, &iso, 0);
    }
}

#[test]
#[ignore = "needs YAOPLM_FAT32_DIR and YAOPLM_EXFAT_DIR: mounted image files"]
fn import_onto_mounted_images() {
    let mut ran = 0;
    for (var, kind) in [
        ("YAOPLM_FAT32_DIR", FsKind::Fat32),
        ("YAOPLM_EXFAT_DIR", FsKind::ExFat),
    ] {
        if let Ok(dir) = std::env::var(var) {
            check(&dir, kind);
            ran += 1;
        }
    }
    assert!(ran > 0, "set YAOPLM_FAT32_DIR and/or YAOPLM_EXFAT_DIR");
}
