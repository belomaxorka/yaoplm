//! SYSTEM.CNF parsing: OPL's BOOT2 path and the lenient info reader.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::system_cnf;

fuzz_target!(|data: &[u8]| {
    if let Ok(boot) = system_cnf::parse_boot2(data) {
        assert!(boot.value.len() <= system_cnf::BOOT_PATH_MAX);
        if let Ok(id) = system_cnf::startup_from_boot_path(&boot) {
            assert!(!id.is_empty() && id.len() <= system_cnf::STARTUP_MAX);
        }
    }
    let _ = system_cnf::parse_info(data);
});
