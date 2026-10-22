//! Property tests: round trips, and parsers that never panic on garbage
//! (TESTING.md "Property / fuzz"). The cargo-fuzz targets in `fuzz/` check
//! the same invariants with coverage guidance.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Cursor;

use proptest::prelude::*;
use yaoplm_formats::cfg::{CfgDocument, KEY_MAX, VALUE_MAX};
use yaoplm_formats::cht::ChtFile;
use yaoplm_formats::image::{self, DiscImage};
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_formats::ul_cfg::{RECORD_SIZE, UlCfg, UlRecord};
use yaoplm_formats::usba_crc32::usba_crc32;
use yaoplm_formats::zso::{self, ZsoReader};
use yaoplm_formats::{Media, system_cnf};

/// Text with the characters that matter to the CFG grammar.
fn cfg_text() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![
            Just(b'='),
            Just(b':'),
            Just(b'#'),
            Just(b' '),
            Just(b'\t'),
            Just(b'\r'),
            Just(b'\n'),
            b'a'..=b'e',
            any::<u8>(),
        ],
        0..300,
    )
}

/// Keys OPL can store: no leading blank, no '=', no line breaks, no '#'.
fn key() -> impl Strategy<Value = String> {
    "[A-Za-z$_][A-Za-z0-9$_ .-]{0,30}".prop_filter("fits", |k| k.len() <= KEY_MAX)
}

fn value() -> impl Strategy<Value = String> {
    "[ -~]{0,64}"
}

proptest! {
    #[test]
    fn cfg_parse_is_lossless(bytes in cfg_text()) {
        prop_assert_eq!(CfgDocument::parse(&bytes).to_bytes(), bytes);
    }

    #[test]
    fn cfg_set_then_get(bytes in cfg_text(), k in key(), v in value()) {
        let mut doc = CfgDocument::parse(&bytes);
        let others: Vec<_> = doc.entries().into_iter().filter(|e| e.key != k).collect();
        doc.set(&k, &v).unwrap();
        prop_assert_eq!(doc.get(&k), Some(v.clone()));
        // Every other key keeps its value and relative order.
        let after: Vec<_> = doc.entries().into_iter().filter(|e| e.key != k).collect();
        prop_assert_eq!(after, others);
        // And a fresh parse of the bytes agrees.
        prop_assert_eq!(CfgDocument::parse(&doc.to_bytes()).get(&k), Some(v));
    }

    #[test]
    fn cfg_written_by_opl_rules_reads_back(pairs in proptest::collection::vec((key(), value()), 0..12)) {
        let mut doc = CfgDocument::new();
        for (k, v) in &pairs {
            doc.set(k, v).unwrap();
        }
        let bytes = doc.to_bytes();
        prop_assert!(bytes.is_empty() || bytes.ends_with(b"\r\n"));
        prop_assert_eq!(CfgDocument::parse(&doc.to_opl_bytes()).entries(), doc.entries());
        for line in bytes.split(|&b| b == b'\n').filter(|l| !l.is_empty()) {
            prop_assert!(line.len() <= KEY_MAX + 1 + VALUE_MAX + 1);
        }
    }

    #[test]
    fn ul_cfg_round_trip(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let table = UlCfg::parse(&bytes);
        let whole = bytes.len() / RECORD_SIZE * RECORD_SIZE;
        prop_assert_eq!(table.to_bytes(), &bytes[..whole]);
        prop_assert_eq!(table.trailing.len(), bytes.len() - whole);
    }

    #[test]
    fn ul_records_built_from_valid_input(name in "[ -~]{1,32}", parts in 1usize..=255, dvd in any::<bool>()) {
        let media = if dvd { Media::Dvd } else { Media::Cd };
        let record = UlRecord::new(&name, "SLUS_200.62", parts, media).unwrap();
        prop_assert!(record.anomalies().is_empty());
        prop_assert_eq!(record.name(), name.clone());
        prop_assert_eq!(record.name_crc(), usba_crc32(name.as_bytes()));
        let reparsed = UlCfg::parse(record.as_bytes());
        prop_assert_eq!(&reparsed.records[0], &record);
    }

    #[test]
    fn zso_round_trip(seed in any::<u64>(), sectors in 1usize..6, zeros in 0usize..3) {
        let mut state = seed | 1;
        let mut iso: Vec<u8> = (0..sectors * SECTOR_SIZE)
            .map(|_| { state ^= state << 13; state ^= state >> 7; state ^= state << 17; (state % 7) as u8 })
            .collect();
        iso.extend(std::iter::repeat_n(0, zeros * SECTOR_SIZE));
        let mut out = Cursor::new(Vec::new());
        zso::compress(&mut Cursor::new(&iso), iso.len() as u64, &mut out, &mut |_| {}).unwrap();
        let mut reader = ZsoReader::new(Cursor::new(out.into_inner())).unwrap();
        let mut sector = [0u8; SECTOR_SIZE];
        for (lba, expected) in iso.chunks(SECTOR_SIZE).enumerate() {
            reader.read_sector(lba as u64, &mut sector).unwrap();
            prop_assert_eq!(&sector[..], expected);
        }
    }

    #[test]
    fn parsers_survive_garbage(bytes in proptest::collection::vec(any::<u8>(), 0..5000)) {
        if let Ok(mut image) = DiscImage::open(Cursor::new(&bytes)) {
            let _ = image::opl_startup(&mut image);
        }
        if let Ok(boot) = system_cnf::parse_boot2(&bytes) {
            let _ = system_cnf::startup_from_boot_path(&boot);
        }
        let _ = system_cnf::parse_info(&bytes);
        let _ = ChtFile::parse(&bytes);
        let _ = yaoplm_formats::games_bin::parse(&bytes);
        let _ = yaoplm_formats::vmc::opl_check(&bytes, bytes.len() as u64);
    }

    #[test]
    fn startup_ids_are_short_and_printable(bytes in cfg_text()) {
        if let Ok(id) = system_cnf::opl_startup(&bytes) {
            prop_assert!(!id.is_empty() && id.len() <= system_cnf::STARTUP_MAX);
            prop_assert!(id.bytes().all(|b| b.is_ascii_graphic()));
        }
    }
}

/// Garbage with a ZISO magic must not make the reader allocate or panic.
#[test]
fn zso_headers_with_huge_sizes() {
    let mut bytes = b"ZISO".to_vec();
    bytes.extend_from_slice(&24u32.to_le_bytes());
    bytes.extend_from_slice(&u64::MAX.to_le_bytes());
    bytes.extend_from_slice(&2048u32.to_le_bytes());
    bytes.extend_from_slice(&[1, 0, 0, 0]);
    bytes.extend_from_slice(&[0xFF; 64]);
    let mut reader = ZsoReader::new(Cursor::new(bytes)).unwrap();
    let mut sector = [0u8; SECTOR_SIZE];
    assert!(reader.read_sector(0, &mut sector).is_err());
    assert!(
        reader
            .read_sector(reader.sector_count() - 1, &mut sector)
            .is_err()
    );
}
