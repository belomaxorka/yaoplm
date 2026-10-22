//! Every fixture variant through the OPL-exact readers, as ISO and as ZSO.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;

use yaoplm_formats::image::{self, Container, DiscImage, StartupError};
use yaoplm_formats::iso9660::{self, Layers, PVD_LBA};
use yaoplm_formats::system_cnf::CnfError;
use yaoplm_formats::zso;
use yaoplm_test_support::Variant;

fn iso(variant: Variant) -> Vec<u8> {
    variant.build().to_vec()
}

fn to_zso(iso: &[u8]) -> Vec<u8> {
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

fn startup(bytes: Vec<u8>) -> (Container, Result<String, StartupError>) {
    let mut image = DiscImage::open(Cursor::new(bytes)).unwrap();
    (image.container(), image::opl_startup(&mut image))
}

#[test]
fn variants_match_opl() {
    for variant in Variant::ALL {
        let iso = iso(variant);
        for (bytes, container) in [
            (iso.clone(), Container::Iso),
            (to_zso(&iso), Container::Zso),
        ] {
            let (detected, result) = startup(bytes);
            assert_eq!(detected, container);
            assert_eq!(
                result.as_deref().ok(),
                variant.expected_startup(),
                "{variant:?} as {container:?}: {result:?}"
            );
        }
    }
}

#[test]
fn hidden_variants_report_the_reason() {
    let reason = |v: Variant| startup(iso(v)).1.unwrap_err();
    assert!(matches!(
        reason(Variant::NoBoot2),
        StartupError::Cnf(CnfError::NoBoot2)
    ));
    assert!(matches!(
        reason(Variant::Ps1),
        StartupError::Cnf(CnfError::NoBoot2)
    ));
    assert!(matches!(
        reason(Variant::LowercaseKey),
        StartupError::Cnf(CnfError::NoBoot2)
    ));
    assert!(matches!(
        reason(Variant::NoVersionSuffix),
        StartupError::NoSystemCnf
    ));
    assert!(matches!(
        reason(Variant::LowercaseName),
        StartupError::NoSystemCnf
    ));
}

#[test]
fn dvd9_layer_break() {
    for bytes in [iso(Variant::Dvd9), to_zso(&iso(Variant::Dvd9))] {
        let mut image = DiscImage::open(Cursor::new(bytes)).unwrap();
        let pvd = iso9660::read_pvd(&mut image, PVD_LBA).unwrap();
        let layer1_start = u32::try_from(Variant::DVD9_LAYER0_SECTORS).unwrap();
        assert_eq!(
            iso9660::opl_layers(&mut image, &pvd),
            Layers::Dual { layer1_start }
        );
    }
    let mut single = DiscImage::open(Cursor::new(iso(Variant::Retail))).unwrap();
    let pvd = iso9660::read_pvd(&mut single, PVD_LBA).unwrap();
    assert_eq!(iso9660::opl_layers(&mut single, &pvd), Layers::Single);
}

#[test]
fn zso_magic_wins_over_extension_like_opl() {
    // OPL probes the content, so a ZSO renamed to .iso still mounts.
    let (container, result) = startup(to_zso(&iso(Variant::Retail)));
    assert_eq!(container, Container::Zso);
    assert_eq!(result.unwrap(), "SLUS_200.62");
}

#[test]
fn not_an_iso() {
    let (_, result) = startup(vec![0u8; 40 * 2048]);
    assert!(matches!(
        result,
        Err(StartupError::Iso(iso9660::IsoError::NoPvd))
    ));
    let (_, result) = startup(b"tiny".to_vec());
    assert!(matches!(
        result,
        Err(StartupError::Iso(iso9660::IsoError::Io(_)))
    ));
}
