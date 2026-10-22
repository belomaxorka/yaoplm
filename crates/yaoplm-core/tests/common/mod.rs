//! Shared fixtures for the engine tests.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use yaoplm_core::exec::{Context, Progress, Report, execute};
use yaoplm_core::plan::Plan;
use yaoplm_device::fs::DeviceFs;
use yaoplm_device::{CancelToken, SourceFs};
use yaoplm_test_support::fixtures::{placeholder_elf, ps2_cnf};
use yaoplm_test_support::memfs::{Data, PROBE_LEN};
use yaoplm_test_support::{Content, IsoBuilder, MemFs, VirtualIso};

pub const GIB: u64 = 1 << 30;

pub fn src(name: &str) -> PathBuf {
    PathBuf::from("src").join(name)
}

/// A game well over 4 GiB whose data differs at every offset, so a chunk
/// cut in the wrong place is caught.
pub fn big_iso(id: &str, pad: u64) -> VirtualIso {
    IsoBuilder::new()
        .system_cnf(ps2_cnf(&format!("cdrom0:\\{id};1")))
        .file(format!("{id};1"), Content::Bytes(placeholder_elf()))
        .file("DATA.BIN;1", Content::Noise { len: pad, seed: 7 })
        .build()
}

pub fn run(plan: &Plan, fs: &dyn DeviceFs, sources: &dyn SourceFs) -> Report {
    execute(
        plan,
        &Context {
            fs,
            sources,
            cancel: &CancelToken::new(),
        },
        &mut |_: &Progress| {},
    )
}

pub fn run_with(
    plan: &Plan,
    fs: &dyn DeviceFs,
    sources: &dyn SourceFs,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(&Progress),
) -> Report {
    execute(
        plan,
        &Context {
            fs,
            sources,
            cancel,
        },
        progress,
    )
}

/// Checks a file the MemFs keeps as probes against the source image from
/// `offset`.
pub fn assert_same_as(fs: &MemFs, path: &str, iso: &VirtualIso, offset: u64) {
    let data = fs.data(path).unwrap_or_else(|| panic!("{path} is missing"));
    let mut iso = iso.clone();
    match data {
        Data::Bytes(bytes) => {
            let mut expected = vec![0u8; bytes.len()];
            iso.seek(SeekFrom::Start(offset)).unwrap();
            iso.read_exact(&mut expected).unwrap();
            assert!(bytes == expected, "{path} differs from the source");
        }
        Data::Sparse { probes, .. } => {
            assert!(!probes.is_empty());
            for (at, bytes) in probes {
                let mut expected = [0u8; PROBE_LEN];
                iso.seek(SeekFrom::Start(offset + at)).unwrap();
                iso.read_exact(&mut expected).unwrap();
                assert_eq!(bytes, expected, "{path} differs at {at}");
            }
        }
    }
}

/// Device paths, without the engine's own bookkeeping folder.
pub fn files(fs: &MemFs) -> Vec<(String, u64)> {
    fs.files()
        .into_iter()
        .filter(|(p, _)| !p.starts_with(".yaoplm/"))
        .collect()
}
