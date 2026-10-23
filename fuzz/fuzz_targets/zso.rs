//! ZSO reading of arbitrary files, the stream decoder against the reader,
//! and compress/read round trips.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::iso9660::{SECTOR_SIZE, SectorSource};
use yaoplm_formats::zso::{self, StreamDecoder, ZsoReader};

/// Piece sizes for the stream decoder: odd ones, some crossing the header,
/// the index and block boundaries.
const PIECES: [usize; 7] = [1, 3, 24, 511, 2048, 2049, 7];

fuzz_target!(|data: &[u8]| {
    if let Ok(mut reader) = ZsoReader::new(Cursor::new(data)) {
        let mut sector = [0u8; SECTOR_SIZE];
        // The header may claim terabytes; a few sectors cover the paths.
        for lba in 0..reader.sector_count().min(64) {
            let _ = reader.read_sector(lba, &mut sector);
        }
    }

    // Every sector the stream decoder hands out, the reader reads the same
    // from the same bytes; once it is done, it handed out all of them.
    let mut decoder = StreamDecoder::new();
    let mut decoded = Vec::new();
    let mut at = 0;
    for size in PIECES.iter().cycle().skip(data.len() % PIECES.len()) {
        if at == data.len() {
            break;
        }
        let end = (at + size).min(data.len());
        if decoder
            .push(&data[at..end], &mut |s| decoded.extend_from_slice(s))
            .is_err()
        {
            break;
        }
        at = end;
    }
    if decoder.is_done() || !decoded.is_empty() {
        let mut reader = ZsoReader::new(Cursor::new(data)).unwrap();
        let mut sector = [0u8; SECTOR_SIZE];
        for (lba, expected) in decoded.chunks(SECTOR_SIZE).enumerate() {
            reader.read_sector(lba as u64, &mut sector).unwrap();
            assert_eq!(&sector[..], expected);
        }
        if decoder.is_done() {
            assert_eq!(decoder.sector_count(), Some(reader.sector_count()));
            assert_eq!(
                decoded.len() as u64,
                reader.sector_count() * SECTOR_SIZE as u64
            );
        }
    }

    let iso = &data[..data.len() / SECTOR_SIZE * SECTOR_SIZE];
    if !iso.is_empty() {
        let mut out = Cursor::new(Vec::new());
        zso::compress(
            &mut Cursor::new(iso),
            iso.len() as u64,
            &mut out,
            &mut |_| {},
        )
        .unwrap();
        let mut reader = ZsoReader::new(Cursor::new(out.into_inner())).unwrap();
        let mut sector = [0u8; SECTOR_SIZE];
        for (lba, expected) in iso.chunks(SECTOR_SIZE).enumerate() {
            reader.read_sector(lba as u64, &mut sector).unwrap();
            assert_eq!(&sector[..], expected);
        }
    }
});
