//! PS2 memory card images (ADR-0048): opening, walking and reading any
//! image never panics or runs away, and every `icon.sys` found parses or is
//! refused. Input that starts with the superblock magic is read as an image;
//! anything else is XORed into a small valid card (64 clusters), so the
//! fuzzer reaches the directory and file code with corruptions of its own.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::icon_sys::IconSys;
use yaoplm_formats::ps2mc::{Card, MAGIC};

const CLUSTER: usize = 1024;
const CLUSTERS: usize = 64;

/// A blank card: superblock, indirect FAT in cluster 8, FAT in 9, the root
/// directory (`.` and `..`) at cluster 10, the first allocatable one.
fn template() -> Vec<u8> {
    let mut card = vec![0xFFu8; CLUSTER * CLUSTERS];
    let put16 = |c: &mut Vec<u8>, at: usize, v: u16| c[at..at + 2].copy_from_slice(&v.to_le_bytes());
    let put32 = |c: &mut Vec<u8>, at: usize, v: u32| c[at..at + 4].copy_from_slice(&v.to_le_bytes());
    card[..0x154].fill(0);
    card[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
    card[28..35].copy_from_slice(b"1.2.0.0");
    put16(&mut card, 0x28, 512);
    put16(&mut card, 0x2A, 2);
    put16(&mut card, 0x2C, 16);
    put32(&mut card, 0x30, CLUSTERS as u32);
    put32(&mut card, 0x34, 10);
    put32(&mut card, 0x38, 50);
    put32(&mut card, 0x3C, 0);
    put32(&mut card, 0x40, 7);
    put32(&mut card, 0x44, 6);
    card[0x50..0xD0].fill(0xFF);
    put32(&mut card, 0x50, 8);
    card[0x150] = 2;
    card[0x151] = 0x2B;
    let ifc = 8 * CLUSTER;
    card[ifc..ifc + CLUSTER].fill(0);
    put32(&mut card, ifc, 9);
    let fat = 9 * CLUSTER;
    for n in 0..256 {
        put32(&mut card, fat + n * 4, if n == 0 { u32::MAX } else { 0x7FFF_FFFF });
    }
    let root = 10 * CLUSTER;
    card[root..root + CLUSTER].fill(0);
    put16(&mut card, root, 0x8427);
    put32(&mut card, root + 4, 2);
    card[root + 0x40] = b'.';
    put16(&mut card, root + 512, 0xA426);
    card[root + 512 + 0x40..root + 512 + 0x42].copy_from_slice(b"..");
    card
}

fuzz_target!(|data: &[u8]| {
    let image = if data.starts_with(MAGIC) {
        data.to_vec()
    } else {
        let mut card = template();
        let len = card.len();
        for (i, b) in data.iter().enumerate() {
            card[i % len] ^= b;
        }
        card
    };
    let len = image.len() as u64;
    let Ok(mut card) = Card::open(Cursor::new(image), len) else {
        return;
    };
    let Ok(walked) = card.walk(8) else {
        return;
    };
    for (_, entry) in walked.iter().filter(|(_, e)| e.is_file()) {
        if let Ok(bytes) = card.read(entry, 1 << 20) {
            assert!(bytes.len() <= entry.length as usize);
            let _ = IconSys::parse(&bytes).map(|icon| icon.title());
        }
    }
});
