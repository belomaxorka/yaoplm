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

/// Writes into a folder on a mounted FAT32 or exFAT image (CI job
/// `real-fs`; never a real drive): one file in a single pass, then two
/// files grown in turns a cluster at a time, so the driver interleaves
/// their clusters. `YAOPLM_FRAGMENTS_KNOWN=1` expects the OS to tell
/// (Windows, Linux as root), `0` expects it not to (Linux without
/// `CAP_SYS_RAWIO`); unset accepts either and prints what it got.
#[test]
#[ignore = "needs YAOPLM_FAT32_DIR or YAOPLM_EXFAT_DIR"]
fn fragments_on_real_drivers() {
    use std::io::Write;

    let known = std::env::var("YAOPLM_FRAGMENTS_KNOWN").ok();
    let dirs: Vec<String> = ["YAOPLM_FAT32_DIR", "YAOPLM_EXFAT_DIR"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .collect();
    assert!(!dirs.is_empty(), "set YAOPLM_FAT32_DIR or YAOPLM_EXFAT_DIR");
    for dir in dirs {
        let root = Path::new(&dir).join("yaoplm-fragments");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let fs = HostFs::new(&root).unwrap();
        let cluster = fs.info().unwrap().cluster_size as usize;

        let mut one = std::fs::File::create(root.join("one.bin")).unwrap();
        one.write_all(&vec![0x5A; cluster * 64]).unwrap();
        one.sync_all().unwrap();
        drop(one);

        let mut a = std::fs::File::create(root.join("a.bin")).unwrap();
        let mut b = std::fs::File::create(root.join("b.bin")).unwrap();
        let chunk = vec![0xA5; cluster];
        for _ in 0..16 {
            a.write_all(&chunk).unwrap();
            a.sync_all().unwrap();
            b.write_all(&chunk).unwrap();
            b.sync_all().unwrap();
        }
        drop((a, b));

        let count = |name: &str| fs.fragments(&RelPath::new(name).unwrap()).unwrap();
        let (one, a, b) = (count("one.bin"), count("a.bin"), count("b.bin"));
        println!("{dir}: cluster {cluster}, one {one:?}, a {a:?}, b {b:?}");
        match known.as_deref() {
            Some("1") => assert!(one.is_some(), "{dir}: the OS should tell"),
            Some("0") => assert!(one.is_none(), "{dir}: the OS should not tell"),
            _ => {}
        }
        if let (Some(one), Some(a), Some(b)) = (one, a, b) {
            assert_eq!(one, 1, "{dir}: one pass on an empty volume is one run");
            assert!(a > 1 && b > 1, "{dir}: interleaved files are in pieces");
        } else {
            assert!(
                one.is_none() && a.is_none() && b.is_none(),
                "{dir}: all or none"
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
