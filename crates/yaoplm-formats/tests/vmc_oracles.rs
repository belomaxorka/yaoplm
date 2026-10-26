//! Blank memory cards read by other tools (VMC-1, research/15 §2.1).
//!
//! - mymcplusplus (GPL-3.0), an independent PS2 memory card file system:
//!   it checks our cards, counts their free space, lists their root and
//!   writes a save directory into one.
//! - ps2vmc-tool's sample cards, made by OPL's genvmc and holding saves:
//!   their superblocks must equal ours.
//!
//! Both live in the reference clones (`$YAOPLM_TESTDATA/refs`, research
//! REFERENCES.md); nothing of them is copied here. Run with
//! `YAOPLM_TESTDATA` set: `cargo test -p yaoplm-formats --test vmc_oracles -- --ignored`.
//! `YAOPLM_PYTHON` picks the Python (3.9 or later).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use yaoplm_formats::vmc::{self, CLUSTER_SIZE, Layout, McTime, SIZES_MB, blank_card};

fn refs() -> PathBuf {
    let dir = std::env::var_os("YAOPLM_TESTDATA").expect("set YAOPLM_TESTDATA");
    PathBuf::from(dir).join("refs")
}

fn python() -> String {
    std::env::var("YAOPLM_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}

/// `mymc <card> <args>`, from the clone, without writing bytecode there.
fn mymc(card: &Path, args: &[&str]) -> (bool, String) {
    let script = "import sys; sys.path.insert(0, sys.argv[1]); \
                  from mymcplusplus import mymc; \
                  sys.exit(mymc.main(['mymc'] + sys.argv[2:]))";
    let output = Command::new(python())
        .args(["-B", "-c", script])
        .arg(refs().join("Adubbz__mymcplusplus"))
        .arg(card)
        .args(args)
        .output()
        .expect("run Python");
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    (output.status.success(), text)
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with the mymcplusplus clone and Python"]
fn mymcplusplus_reads_and_writes_our_cards() {
    let dir = tempfile::tempdir().unwrap();
    for mb in SIZES_MB {
        let path = dir.path().join(format!("card{mb}.bin"));
        std::fs::write(
            &path,
            blank_card(mb, McTime::from_unix(1_790_000_000)).unwrap(),
        )
        .unwrap();
        let layout = Layout::new(mb).unwrap();

        let (ok, out) = mymc(&path, &["check"]);
        assert!(ok && out.contains("No errors found"), "{mb} MiB: {out}");
        // Every allocatable cluster but the root directory's is free.
        let free = (u64::from(layout.alloc_end) - 1) * CLUSTER_SIZE as u64;
        let (ok, out) = mymc(&path, &["df"]);
        assert!(
            ok && out.contains(&format!("{free} bytes free")),
            "{mb} MiB: {out}"
        );
        let (ok, out) = mymc(&path, &["ls"]);
        let names: Vec<&str> = out
            .lines()
            .filter_map(|l| l.split_whitespace().last())
            .collect();
        assert!(ok && names == [".", ".."], "{mb} MiB: {out}");

        // A save directory goes in, and the card is still sound.
        let (ok, out) = mymc(&path, &["mkdir", "BASLUS-20062YAOPLM"]);
        assert!(ok, "{mb} MiB mkdir: {out}");
        let (ok, out) = mymc(&path, &["check"]);
        assert!(
            ok && out.contains("No errors found"),
            "{mb} MiB after mkdir: {out}"
        );
        let card = std::fs::read(&path).unwrap();
        assert_eq!(card.len() as u64, u64::from(mb) << 20, "mymc kept the size");
        assert_eq!(vmc::opl_check(&card, card.len() as u64), Ok(mb));
        let root = vmc::Superblock::parse(&card).root_dir_offset() as usize;
        assert_eq!(vmc::directory_entries(&card[root..]), Some(3));
    }
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with the ps2vmc-tool clone"]
fn genvmc_samples_have_our_superblock() {
    let samples = refs().join("bucanero__ps2vmc-tool").join("samples");
    for (file, mb) in [
        ("oplcard.bin", 8),
        ("card16mb.bin", 16),
        ("card32mb.bin", 32),
    ] {
        let sample = std::fs::read(samples.join(file)).unwrap();
        assert_eq!(sample.len() as u64, u64::from(mb) << 20, "{file}");
        let ours = blank_card(mb, McTime::FALLBACK).unwrap();
        assert_eq!(
            sample[..CLUSTER_SIZE],
            ours[..CLUSTER_SIZE],
            "{file}: superblock cluster"
        );
        // Their saves live in the root directory OPL's way.
        let root = vmc::Superblock::parse(&sample).root_dir_offset() as usize;
        assert!(
            vmc::directory_entries(&sample[root..]).is_some_and(|n| n > 2),
            "{file}"
        );
    }
}
