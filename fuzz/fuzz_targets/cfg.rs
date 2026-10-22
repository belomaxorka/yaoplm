//! CFG parsing must be lossless, and OPL's writer output must read back to
//! the same keys.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::cfg::{self, CfgDocument};

fuzz_target!(|data: &[u8]| {
    let doc = CfgDocument::parse(data);
    assert_eq!(doc.to_bytes(), data);
    let _ = cfg::detect_dialect(data);

    let before: Vec<_> = doc.entries().into_iter().filter(|e| !e.key.starts_with('#')).collect();
    // OPL itself does not read these back the same way: leading blanks
    // (from an indented prefix) are skipped, and a leading BOM is dropped.
    let unstable = |key: &str| key.starts_with([' ', '\t', '\u{feff}']);
    if before.iter().any(|e| unstable(&e.key)) {
        return;
    }
    let written = CfgDocument::parse(&doc.to_opl_bytes());
    assert_eq!(written.entries(), before);
});
