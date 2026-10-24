//! Loader ELFs: reading arbitrary files never panics, and a version found is
//! always a well-formed one.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::ps2_elf;

fuzz_target!(|data: &[u8]| {
    if let Ok(program) = ps2_elf::program(data) {
        // The decoder never makes more than the EE's RAM.
        assert!(program.bytes.len() <= 32 << 20);
        if let Some(version) = ps2_elf::find_version(&program.bytes) {
            assert!(version.starts_with('v') && version.len() <= 80);
        }
    }
});
