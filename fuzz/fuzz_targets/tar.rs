//! Picture sets in tar archives: the reader never panics, never hands out
//! more bytes than a file claims, and the picture-name parser takes any
//! name it finds.
#![no_main]

use std::io::Read;

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::art::pack_picture;
use yaoplm_formats::tar::for_each_file;

fuzz_target!(|data: &[u8]| {
    let _ = for_each_file(data, &mut |entry, file| {
        let _ = pack_picture(&entry.name);
        let mut buf = Vec::new();
        let read = file.take(1 << 20).read_to_end(&mut buf)?;
        assert!(read as u64 <= entry.size);
        Ok(())
    });
});
