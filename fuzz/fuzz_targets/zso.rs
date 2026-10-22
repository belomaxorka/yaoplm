//! ZSO reading of arbitrary files, and compress/read round trips.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_formats::zso::{self, ZsoReader};

fuzz_target!(|data: &[u8]| {
    if let Ok(mut reader) = ZsoReader::new(Cursor::new(data)) {
        let mut sector = [0u8; SECTOR_SIZE];
        // The header may claim terabytes; a few sectors cover the paths.
        for lba in 0..reader.sector_count().min(64) {
            let _ = reader.read_sector(lba, &mut sector);
        }
    }

    let iso = &data[..data.len() / SECTOR_SIZE * SECTOR_SIZE];
    if !iso.is_empty() {
        let mut out = Cursor::new(Vec::new());
        zso::compress(&mut Cursor::new(iso), iso.len() as u64, &mut out, &mut |_| {}).unwrap();
        let mut reader = ZsoReader::new(Cursor::new(out.into_inner())).unwrap();
        let mut sector = [0u8; SECTOR_SIZE];
        for (lba, expected) in iso.chunks(SECTOR_SIZE).enumerate() {
            reader.read_sector(lba as u64, &mut sector).unwrap();
            assert_eq!(&sector[..], expected);
        }
    }
});
