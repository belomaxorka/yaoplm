//! Arbitrary bytes as a disc image: probing, PVD, OPL's root lookup,
//! SYSTEM.CNF, the layer probe and a standard directory listing.
#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use yaoplm_formats::image::{self, DiscImage};
use yaoplm_formats::iso9660::{self, PVD_LBA};

fuzz_target!(|data: &[u8]| {
    let Ok(mut image) = DiscImage::open(Cursor::new(data)) else {
        return;
    };
    let _ = image::opl_startup(&mut image);
    if let Ok(pvd) = iso9660::read_pvd(&mut image, PVD_LBA) {
        let _ = iso9660::opl_layers(&mut image, &pvd);
        let _ = iso9660::read_dir(&mut image, &pvd.root);
    }
});
