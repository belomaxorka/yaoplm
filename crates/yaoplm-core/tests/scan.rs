//! The scanner against a library built in a temp dir from fixtures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::Path;

use yaoplm_core::{
    GameFormat, GameLocation, LoaderProfile, Media, ScanOptions, SkipReason, Warning, image_info,
    scan_library,
};
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_formats::{iso9660::Layers, zso};
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

fn games_bin_record(name: &str, startup: &str, ext: &str) -> Vec<u8> {
    let mut r = vec![0u8; 188];
    r[..name.len()].copy_from_slice(name.as_bytes());
    r[161..161 + startup.len()].copy_from_slice(startup.as_bytes());
    r[174..174 + ext.len()].copy_from_slice(ext.as_bytes());
    r
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    png.extend_from_slice(&width.to_be_bytes());
    png.extend_from_slice(&height.to_be_bytes());
    png.extend_from_slice(&[8, 6, 0, 0, 0]);
    png
}

fn vmc(size_mb: u32) -> Vec<u8> {
    let mut card = vec![0xFFu8; (size_mb as usize) << 20];
    card[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
    card[0x28..0x2A].copy_from_slice(&512u16.to_le_bytes());
    card[0x2A..0x2C].copy_from_slice(&2u16.to_le_bytes());
    card[0x30..0x34].copy_from_slice(&(size_mb * 1024).to_le_bytes());
    card[0x150] = 2;
    card
}

/// A device root with one example of every case the scanner handles.
fn build_library(root: &Path) {
    let retail = Variant::Retail.build().to_vec();
    write(
        &root.join("CD/SLUS_200.62.Grand Theft Auto III.iso"),
        &retail,
    );
    write(&root.join("CD/GTA Copy.iso"), &retail);
    write(
        &root.join("DVD/Okami.ZSO"),
        zso_of(&Variant::LfOnly.build().to_vec()),
    );
    write(&root.join("DVD/Dual.iso"), Variant::Dvd9.build().to_vec());
    write(
        &root.join("DVD/No Boot.iso"),
        Variant::NoBoot2.build().to_vec(),
    );
    write(
        &root.join("DVD/Stale.iso"),
        Variant::LongId.build().to_vec(),
    );
    write(
        &root.join("DVD/games.bin"),
        games_bin_record("Stale", "SLUS_999.99", ".iso"),
    );
    write(&root.join("DVD/Compressed.cso"), b"CISO");
    write(&root.join("DVD/readme.txt"), b"not a game");

    // ul: one complete game, one with a missing part, one with a short part.
    let one = UlRecord::new("Ul Game", "SLUS_200.62", 1, Media::Dvd).unwrap();
    let missing = UlRecord::new("Missing Part", "SLES_111.11", 2, Media::Dvd).unwrap();
    let short = UlRecord::new("Short Part", "SLES_222.22", 2, Media::Cd).unwrap();
    write(&root.join(one.chunk_name(0)), &retail);
    write(&root.join(missing.chunk_name(0)), b"x");
    write(&root.join(short.chunk_name(0)), b"x");
    write(&root.join(short.chunk_name(1)), b"x");
    let table = UlCfg {
        records: vec![one, missing, short],
        ..UlCfg::default()
    };
    let mut bytes = table.to_bytes();
    bytes.extend_from_slice(b"junk");
    write(&root.join("ul.cfg"), bytes);

    write(
        &root.join("CFG/SLUS_200.62.cfg"),
        "$Compatibility=65\r\n$VMC_0=nowhere\r\n$VMC_1=generic_0\r\nTitle=GTA III\r\n",
    );
    write(
        &root.join("CFG/SCES_500.03.cfg"),
        "compat :\n{\n  mode = 3;\n};\n",
    );
    write(&root.join("VMC/generic_0.bin"), vmc(8));
    write(&root.join("ART/SLUS_200.62_COV.png"), png(140, 200));
    write(&root.join("ART/slus_200.62_ICO.png"), png(2048, 64));
    write(&root.join("ART/SLUS_200.62_COV2.jpg"), b"\xFF\xD8\xFF");
    write(
        &root.join("CHT/SLUS_200.62.cht"),
        "Mastercode\n9029A2A8 0C0A6A1E\nCash\n2032D3B4 00000063\n",
    );

    write(
        &root.join("APPS/WLE/title.cfg"),
        "title=wLaunchELF\r\nboot=WLE.ELF\r\n",
    );
    write(&root.join("APPS/WLE/WLE.ELF"), b"\x7FELF");
    write(&root.join("APPS/Broken/title.cfg"), "title=No boot key\r\n");
    write(
        &root.join("APPS/Lost/title.cfg"),
        "title=Lost\r\nboot=GONE.ELF\r\n",
    );
    fs::create_dir_all(root.join("APPS/NotAnApp")).unwrap();
    fs::create_dir_all(root.join("THM")).unwrap();
}

#[test]
fn scans_like_opl() {
    let dir = tempfile::tempdir().unwrap();
    build_library(dir.path());
    let lib = scan_library(dir.path(), &ScanOptions::default()).unwrap();

    let games: Vec<(&str, &str, Media, GameFormat)> = lib
        .games
        .iter()
        .map(|g| (g.title.as_str(), g.startup.as_str(), g.media, g.format))
        .collect();
    assert_eq!(
        games,
        [
            ("Dual", "SLUS_212.18", Media::Dvd, GameFormat::Iso),
            (
                "Grand Theft Auto III",
                "SLUS_200.62",
                Media::Cd,
                GameFormat::Iso
            ),
            ("GTA Copy", "SLUS_200.62", Media::Cd, GameFormat::Iso),
            ("Missing Part", "SLES_111.11", Media::Dvd, GameFormat::Ul),
            ("Okami", "SCES_500.03", Media::Dvd, GameFormat::Zso),
            ("Short Part", "SLES_222.22", Media::Cd, GameFormat::Ul),
            ("Stale", "SLUS_999.99", Media::Dvd, GameFormat::Iso),
            ("Ul Game", "SLUS_200.62", Media::Dvd, GameFormat::Ul),
        ]
    );
    let game = |title: &str| lib.games.iter().find(|g| g.title == title).unwrap();

    // games.bin wins over the image, as in OPL.
    assert!(game("Stale").warnings.contains(&Warning::StaleCache {
        cached: "SLUS_999.99".into(),
        image: "SLPM_123.45".into()
    }));
    assert_eq!(game("Dual").layers, Some(Layers::Dual { layer1_start: 64 }));
    assert!(
        game("Okami")
            .warnings
            .contains(&Warning::UppercaseExtension)
    );

    let gta = game("Grand Theft Auto III");
    assert!(
        matches!(&gta.location, GameLocation::Image { file_name, .. } if file_name.starts_with("SLUS_200.62."))
    );
    assert!(
        gta.warnings
            .contains(&Warning::DuplicateStartup("SLUS_200.62".into()))
    );
    assert!(gta.warnings.contains(&Warning::CompatibilityBits {
        value: 65,
        allowed: 0x3F
    }));
    assert!(gta.warnings.contains(&Warning::VmcMissing {
        slot: 0,
        name: "nowhere".into()
    }));
    assert!(
        !gta.warnings
            .iter()
            .any(|w| matches!(w, Warning::VmcInvalid { .. }))
    );
    assert!(
        gta.warnings
            .contains(&Warning::ArtNotLoaded("SLUS_200.62_COV2.jpg".into()))
    );
    assert!(
        gta.warnings
            .iter()
            .any(|w| matches!(w, Warning::ArtTooLarge { width: 2048, .. }))
    );
    let cfg = gta.cfg.as_ref().unwrap();
    assert_eq!(cfg.compatibility, Some(65));
    assert_eq!(cfg.vmc, [Some("nowhere".into()), Some("generic_0".into())]);
    let mut suffixes: Vec<&str> = gta.art.iter().map(|a| a.suffix.as_str()).collect();
    suffixes.sort_unstable();
    assert_eq!(suffixes, ["COV", "COV2", "ICO"]);
    assert_eq!(gta.cheats.as_ref().unwrap().cheats, 2);

    assert!(game("Okami").warnings.contains(&Warning::CfgLibconfig));

    let ul = game("Ul Game");
    assert_eq!(ul.image_startup.as_deref(), Some("SLUS_200.62"));
    assert!(matches!(&ul.location, GameLocation::Ul { index: 0, parts } if parts.len() == 1));
    assert!(
        game("Missing Part")
            .warnings
            .iter()
            .any(|w| matches!(w, Warning::UlPartMissing { part: 1, .. }))
    );
    assert!(game("Short Part").warnings.iter().any(|w| matches!(
        w,
        Warning::UlPartSize {
            part: 0,
            size: 1,
            ..
        }
    )));
    assert!(lib.warnings.contains(&Warning::UlCfgTrailingBytes(4)));

    let skipped: Vec<(String, &SkipReason)> = lib
        .skipped
        .iter()
        .map(|s| {
            (
                s.path.file_name().unwrap().to_string_lossy().into_owned(),
                &s.reason,
            )
        })
        .collect();
    assert_eq!(skipped.len(), 3, "{skipped:?}");
    assert!(skipped.iter().any(|(n, r)| n == "Compressed.cso"
        && matches!(r, SkipReason::UnsupportedExtension(e) if e == ".cso")));
    assert!(
        skipped
            .iter()
            .any(|(n, r)| n == "No Boot.iso" && matches!(r, SkipReason::NoStartup(_)))
    );
    assert!(
        skipped
            .iter()
            .any(|(n, r)| n == "Broken" && matches!(r, SkipReason::BadApp(_)))
    );

    let apps: Vec<(&str, bool)> = lib
        .apps
        .iter()
        .map(|a| (a.title.as_str(), a.boot_found))
        .collect();
    assert_eq!(apps, [("Lost", false), ("wLaunchELF", true)]);

    let folders: Vec<(&str, bool)> = lib.folders.iter().map(|(f, p)| (f.as_str(), *p)).collect();
    assert!(folders.contains(&("THM", true)));
    assert!(folders.contains(&("LNG", false)));
}

#[test]
fn without_verification_opl_shortcuts_are_trusted() {
    let dir = tempfile::tempdir().unwrap();
    build_library(dir.path());
    let options = ScanOptions {
        verify_images: false,
        ..ScanOptions::default()
    };
    let lib = scan_library(dir.path(), &options).unwrap();
    let stale = lib.games.iter().find(|g| g.title == "Stale").unwrap();
    assert_eq!(stale.startup, "SLUS_999.99");
    assert_eq!(stale.image_startup, None);
    assert!(stale.warnings.is_empty());
}

#[test]
fn riptopl_allows_compat_bits_7_and_8() {
    let dir = tempfile::tempdir().unwrap();
    build_library(dir.path());
    let options = ScanOptions {
        profile: LoaderProfile::RiptOpl,
        ..ScanOptions::default()
    };
    let lib = scan_library(dir.path(), &options).unwrap();
    let gta = lib
        .games
        .iter()
        .find(|g| g.title == "Grand Theft Auto III")
        .unwrap();
    assert!(
        !gta.warnings
            .iter()
            .any(|w| matches!(w, Warning::CompatibilityBits { .. }))
    );
}

#[test]
fn not_a_folder() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.iso");
    fs::write(&file, b"x").unwrap();
    assert!(scan_library(&file, &ScanOptions::default()).is_err());
    let empty = scan_library(dir.path(), &ScanOptions::default()).unwrap();
    assert!(empty.games.is_empty());
}

#[test]
fn image_details() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("SLUS_999.99.Mislabeled.iso");
    fs::write(&path, Variant::Retail.build().to_vec()).unwrap();
    let info = image_info(&path, LoaderProfile::Opl110).unwrap();
    assert_eq!(info.format, GameFormat::Iso);
    assert_eq!(info.startup.as_deref(), Ok("SLUS_200.62"));
    assert_eq!(info.suggested_media, Media::Cd);
    assert_eq!(info.cnf.vmode.as_deref(), Some("NTSC"));
    assert_eq!(info.volume_id.as_deref(), Some("YAOPLM_TEST"));
    assert!(info.warnings.contains(&Warning::NameIdMismatch {
        name: "SLUS_999.99".into(),
        image: "SLUS_200.62".into()
    }));

    let zso_path = dir.path().join("game.zso");
    fs::write(&zso_path, zso_of(&Variant::Ps1.build().to_vec())).unwrap();
    let info = image_info(&zso_path, LoaderProfile::Opl110).unwrap();
    assert_eq!(info.format, GameFormat::Zso);
    assert_eq!(info.platform, yaoplm_core::Platform::Ps1);
    assert!(info.startup.is_err());
    assert!(info.warnings.contains(&Warning::ZsoUnsupportedByProfile));
}
