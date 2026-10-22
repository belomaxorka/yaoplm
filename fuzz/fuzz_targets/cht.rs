//! Cheat files: the parser never panics and respects OPL's limits.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::cht::{ChtFile, MAX_ACTIVE_PAIRS, MAX_CHEATS, NAME_MAX};

fuzz_target!(|data: &[u8]| {
    let file = ChtFile::parse(data);
    assert!(file.cheats.len() <= MAX_CHEATS);
    assert!(file.cheats.iter().all(|c| c.name.len() <= NAME_MAX * 3));
    let (codes, _) = file.active_codes(|_| true);
    assert!(codes.len() <= MAX_ACTIVE_PAIRS);
});
