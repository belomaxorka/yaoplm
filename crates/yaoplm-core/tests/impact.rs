//! What replacing the loader changes for the games (LDR-7, ADR-0046): the
//! library scanned with the new build's profile, sorted by what the
//! replace plan can do.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use yaoplm_core::loaders::impact::impact;
use yaoplm_core::{LoaderProfile, ScanOptions, scan_library};
use yaoplm_device::FsKind;
use yaoplm_formats::zso;
use yaoplm_test_support::Variant;

fn write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
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

/// A ZSO game with a JPG cover, an ISO game with a PNG one.
fn library() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let iso = Variant::Retail.build().to_vec();
    write(&root.join("DVD/SLUS_200.62.Retail Game.zso"), zso_of(&iso));
    write(
        &root.join("CD/SLUS_212.18.Lf Game.iso"),
        Variant::LfOnly.build().to_vec(),
    );
    write(
        &root.join("ART/SLUS_200.62_COV.jpg"),
        b"\xff\xd8\xff\xe0 jpeg",
    );
    write(&root.join("ART/SLUS_212.18_COV.png"), b"\x89PNG\r\n\x1a\n");
    dir
}

fn scanned(root: &Path, profile: LoaderProfile) -> yaoplm_core::Library {
    scan_library(
        root,
        &ScanOptions {
            profile,
            verify_images: false,
        },
    )
    .unwrap()
}

#[test]
fn moving_to_opl_110_lists_zso_to_convert() {
    let dir = library();
    let to = LoaderProfile::Opl110;
    let found = impact(
        &scanned(dir.path(), to),
        FsKind::Fat32,
        LoaderProfile::Opl120Beta,
        to,
    );
    assert_eq!(found.blocked, None);
    let zso: Vec<(&str, Option<&Path>)> = found
        .zso
        .iter()
        .map(|g| (g.startup.as_str(), g.path.as_deref()))
        .collect();
    assert_eq!(
        zso,
        [(
            "SLUS_200.62",
            Some(PathBuf::from("DVD/SLUS_200.62.Retail Game.zso").as_path())
        )]
    );
    // 1.1.0 loads JPG: nothing to re-encode.
    assert!(found.art.is_empty());
    assert!(!found.is_empty());
}

#[test]
fn opl_110_on_exfat_is_refused() {
    let dir = library();
    let to = LoaderProfile::Opl110;
    let found = impact(
        &scanned(dir.path(), to),
        FsKind::ExFat,
        LoaderProfile::Opl120Beta,
        to,
    );
    assert_eq!(
        found.blocked.as_ref().map(|p| p.key.as_str()),
        Some("notices.replaceExfatOpl110")
    );
    assert!(found.zso.is_empty() && found.art.is_empty());
}

#[test]
fn leaving_opl_110_lists_pictures_to_re_encode() {
    let dir = library();
    let to = LoaderProfile::Opl120Beta;
    let found = impact(
        &scanned(dir.path(), to),
        FsKind::Fat32,
        LoaderProfile::Opl110,
        to,
    );
    assert!(found.zso.is_empty(), "the betas read ZSO");
    let art: Vec<(&str, &str, PathBuf)> = found
        .art
        .iter()
        .map(|a| (a.startup.as_str(), a.suffix.as_str(), a.path.clone()))
        .collect();
    assert_eq!(
        art,
        [(
            "SLUS_200.62",
            "COV",
            PathBuf::from("ART/SLUS_200.62_COV.jpg")
        )]
    );
}

#[test]
fn the_same_rules_change_nothing() {
    let dir = library();
    let to = LoaderProfile::Opl120Beta;
    let found = impact(
        &scanned(dir.path(), to),
        FsKind::ExFat,
        LoaderProfile::Opl120Beta,
        to,
    );
    assert!(found.is_empty());
}
