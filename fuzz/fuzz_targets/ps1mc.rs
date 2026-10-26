//! PS1 memory card images (ADR-0048): a card of any content parses without
//! a panic, and every save's chain stays on the card and visits a block
//! once. The input is XORed into a formatted card: images are 128 KiB.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::ps1mc::{CARD_SIZE, Ps1Card, SLOTS};

fuzz_target!(|data: &[u8]| {
    let mut card = vec![0u8; CARD_SIZE];
    card[..2].copy_from_slice(b"MC");
    for i in 0..SLOTS {
        let frame = 128 * (i + 1);
        card[frame] = 0xA0;
        card[frame + 8] = 0xFF;
        card[frame + 9] = 0xFF;
        card[frame + 127] = 0xA0;
    }
    for (i, b) in data.iter().enumerate() {
        card[i % CARD_SIZE] ^= b;
    }
    if let Ok(parsed) = Ps1Card::parse(&card) {
        for save in &parsed.saves {
            assert!(!save.blocks.is_empty() && save.blocks.len() <= SLOTS);
            assert!(save.blocks.iter().all(|&b| (1..=SLOTS as u8).contains(&b)));
            let mut seen = save.blocks.clone();
            seen.sort_unstable();
            seen.dedup();
            assert_eq!(seen.len(), save.blocks.len());
        }
    }
});
