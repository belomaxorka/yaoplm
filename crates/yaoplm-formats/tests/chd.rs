//! CHD images read as ISO sectors (IMP-1b), on fixtures written the way
//! chdman lays them out: DVD units, CD frames in every track mode, a CD
//! with an audio track after the data.

#![cfg(feature = "chd")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;

use yaoplm_formats::chd::{ChdError, ChdImage, Layout};
use yaoplm_formats::cue::TrackMode;
use yaoplm_formats::image;
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_test_support::Variant;
use yaoplm_test_support::bincue::RawMode;
use yaoplm_test_support::packed::{chd_cd, chd_dvd};

fn sectors(image: &mut impl SectorSource) -> Vec<u8> {
    let mut out = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    for lba in 0..image.sector_count() {
        image.read_sector(lba, &mut sector).unwrap();
        out.extend_from_slice(&sector);
    }
    out
}

#[test]
fn dvd_chd_is_the_iso() {
    let iso = Variant::Dvd9.build().to_vec();
    let mut chd = ChdImage::open(Cursor::new(chd_dvd(&iso).unwrap())).unwrap();
    assert!(matches!(chd.layout(), Layout::Dvd { .. }));
    assert_eq!(sectors(&mut chd), iso);
    assert_eq!(image::opl_startup(&mut chd).unwrap(), "SLUS_212.18");
}

#[test]
fn cd_chd_in_every_track_mode() {
    let iso = Variant::Retail.build().to_vec();
    for (raw, mode) in [
        (RawMode::Mode1_2048, TrackMode::Mode1_2048),
        (RawMode::Mode1_2352, TrackMode::Mode1_2352),
        (RawMode::Mode2_2336, TrackMode::Mode2_2336),
        (RawMode::Mode2_2352, TrackMode::Mode2_2352),
    ] {
        let mut chd = ChdImage::open(Cursor::new(chd_cd(&iso, raw, 0).unwrap())).unwrap();
        match chd.layout() {
            Layout::Cd {
                mode: found,
                dropped,
                ..
            } => {
                assert_eq!(*found, mode);
                assert!(dropped.is_empty());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(sectors(&mut chd), iso, "{raw:?}");
    }
}

#[test]
fn audio_tracks_are_dropped() {
    let iso = Variant::Retail.build().to_vec();
    let mut chd =
        ChdImage::open(Cursor::new(chd_cd(&iso, RawMode::Mode2_2352, 301).unwrap())).unwrap();
    let Layout::Cd {
        dropped,
        sectors: n,
        ..
    } = chd.layout().clone()
    else {
        panic!("a CD");
    };
    assert_eq!(dropped.len(), 1);
    assert_eq!(dropped[0].kind, "AUDIO");
    assert_eq!(n, iso.len() as u64 / 2048);
    assert_eq!(sectors(&mut chd), iso);
}

#[test]
fn not_a_chd() {
    let err = ChdImage::open(Cursor::new(vec![0u8; 8192])).unwrap_err();
    assert!(matches!(err, ChdError::Chd(_)), "{err}");
}
