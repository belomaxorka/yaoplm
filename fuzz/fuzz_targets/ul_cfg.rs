//! ul.cfg: records round-trip byte for byte; names and checks never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::ul_cfg::{RECORD_SIZE, UlCfg};

fuzz_target!(|data: &[u8]| {
    let table = UlCfg::parse(data);
    let whole = data.len() / RECORD_SIZE * RECORD_SIZE;
    assert_eq!(table.to_bytes(), &data[..whole]);
    for record in &table.records {
        let _ = record.anomalies();
        let _ = record.chunk_names();
    }
});
