//! Fragment counts of real files, read only (VMC-1, IMP-7b): set
//! `YAOPLM_FRAGMENT_FILES` to `;`-separated paths, e.g. the parts of a
//! game on a FAT32 stick, and compare with `fsutil file queryextents`.
//! `cargo test -p yaoplm-device --test fragments -- --ignored --nocapture`

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use yaoplm_device::{DeviceFs, HostFs, RelPath};

#[test]
#[ignore = "needs YAOPLM_FRAGMENT_FILES"]
fn fragments_of_real_files() {
    let list = std::env::var("YAOPLM_FRAGMENT_FILES").expect("set YAOPLM_FRAGMENT_FILES");
    for path in list.split(';').filter(|p| !p.is_empty()) {
        let path = Path::new(path);
        let fs = HostFs::new(path.parent().unwrap()).unwrap();
        let name = path.file_name().unwrap().to_string_lossy();
        let runs = fs.fragments(&RelPath::new(&name).unwrap()).unwrap();
        println!("{}: {runs:?}", path.display());
        if cfg!(windows) {
            assert!(runs.is_some_and(|n| n >= 1), "{}", path.display());
        }
    }
}
