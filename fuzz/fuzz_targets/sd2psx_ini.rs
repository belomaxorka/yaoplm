//! A memory card emulator's ini files (ADR-0048): any bytes parse without a
//! panic, and what the firmware would take is always a value it accepts.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::sd2psx_ini::{CardIni, Ini, settings_card_size};

fuzz_target!(|data: &[u8]| {
    let ini = Ini::parse(data);
    if let Some(size) = settings_card_size(&ini) {
        assert!([1, 2, 4, 8, 16, 32, 64, 128].contains(&size));
    }
    let card = CardIni::read(&ini);
    if let Some(max) = card.max_channels {
        assert!((1..=255).contains(&max));
    }
    assert!(card.names.windows(2).all(|w| w[0].0 < w[1].0));
    for entry in &ini.entries {
        assert!(entry.line >= 1);
    }
    let _ = ini.sections();
});
