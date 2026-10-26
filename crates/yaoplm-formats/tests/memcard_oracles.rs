//! The memory card reader against mymcplusplus on real cards (VMC-2,
//! ADR-0048): the owner's factory SD (research/19), whose boot cards hold
//! FreeMcBoot, PS2BBL and OPL builds, and ps2vmc-tool's sample cards made
//! by OPL's genvmc.
//!
//! Every directory our reader lists must list the same in mymcplusplus
//! (`ls`: mode bits, length, name), and every file up to 2 MiB must read
//! the same bytes as mymcplusplus extracts. The images are copied into a
//! temp folder first: nothing in the test data is opened for writing.
//!
//! Run with `YAOPLM_TESTDATA` set:
//! `cargo test -p yaoplm-formats --test memcard_oracles -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;

use yaoplm_formats::ps2mc::{Card, Entry};

fn testdata() -> PathBuf {
    PathBuf::from(std::env::var_os("YAOPLM_TESTDATA").expect("set YAOPLM_TESTDATA"))
}

fn python() -> String {
    std::env::var("YAOPLM_PYTHON")
        .unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into())
}

fn mymc(card: &Path, args: &[&str]) -> String {
    let script = "import sys; sys.path.insert(0, sys.argv[1]); \
                  from mymcplusplus import mymc; \
                  sys.exit(mymc.main(['mymc'] + sys.argv[2:]))";
    let output = Command::new(python())
        .args(["-B", "-c", script])
        .arg(testdata().join("refs").join("Adubbz__mymcplusplus"))
        .arg(card)
        .args(args)
        .env("TZ", "UTC")
        .output()
        .expect("run Python");
    assert!(
        output.status.success(),
        "mymc {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// mymc's `ls` columns: mode bits, length, name (the date is local time).
fn mode_bits(mode: u16) -> String {
    let bits = b"rwxpfdD81C+KPH4";
    (0..15)
        .map(|bit| {
            if mode & (1 << bit) != 0 {
                bits[bit] as char
            } else {
                '-'
            }
        })
        .collect()
}

fn ours(entries: &[Entry]) -> Vec<String> {
    let mut lines: Vec<String> = entries
        .iter()
        .filter(|e| e.exists())
        .map(|e| format!("{} {} {}", mode_bits(e.mode), e.length, e.name_lossy()))
        .collect();
    lines.sort();
    lines
}

fn theirs(listing: &str) -> Vec<String> {
    let mut lines: Vec<String> = listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            // `%s %7d %04d-%02d-%02d %02d:%02d:%02d %s`: names keep their
            // spaces, so the name is everything after the time.
            let (mode, rest) = l.split_at(15);
            let rest = rest.trim_start();
            let digits = rest.find(' ').unwrap();
            let (length, rest) = rest.split_at(digits);
            let name = &rest[1 + 10 + 1 + 8 + 1..];
            format!("{mode} {length} {name}")
        })
        .collect();
    lines.sort();
    lines
}

fn compare(image: &Path) -> (usize, usize) {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("card.mcd");
    std::fs::copy(image, &copy).unwrap();
    let len = std::fs::metadata(&copy).unwrap().len();
    let mut card = Card::open(File::open(&copy).unwrap(), len).unwrap();
    let root = card.root().unwrap();
    assert_eq!(
        ours(&root),
        theirs(&mymc(&copy, &["ls", "/"])),
        "{image:?} /"
    );
    let mut dirs = 0;
    let mut files = 0;
    for (path, entry) in card.walk(8).unwrap() {
        if entry.is_dir() {
            let listed = card.list(&entry).unwrap();
            assert_eq!(
                ours(&listed),
                theirs(&mymc(&copy, &["ls", &format!("/{path}")])),
                "{image:?} {path}"
            );
            dirs += 1;
        } else if entry.is_file() && entry.length <= 2 << 20 {
            let bytes = card.read(&entry, 2 << 20).unwrap();
            let out = dir.path().join("extracted");
            let _ = std::fs::remove_file(&out);
            mymc(
                &copy,
                &["extract", "-o", out.to_str().unwrap(), &format!("/{path}")],
            );
            assert!(std::fs::read(&out).unwrap() == bytes, "{image:?} {path}");
            files += 1;
        }
    }
    (dirs, files)
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with the factory SD's copy, the clones and Python"]
fn the_factory_cards_read_as_mymcplusplus_reads_them() {
    let sd = testdata()
        .join("backups")
        .join("psxmemcard-gen2-sd-2026-10-26")
        .join("files")
        .join("MemoryCards");
    let mut images: Vec<PathBuf> = (1..=6)
        .map(|n| {
            sd.join("PS2")
                .join("BOOT")
                .join(format!("BootCard-{n}.mcd"))
        })
        .collect();
    images.push(sd.join("PS2").join("Card1").join("Card1-1.mcd"));
    images.push(sd.join("COH").join("BOOT").join("BootCard-1.mcd"));
    for image in images {
        let (dirs, files) = compare(&image);
        println!("{}: {dirs} folders, {files} files match", image.display());
    }
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with the ps2vmc-tool clone and Python"]
fn genvmc_cards_with_saves_read_as_mymcplusplus_reads_them() {
    let samples = testdata()
        .join("refs")
        .join("bucanero__ps2vmc-tool")
        .join("samples");
    for name in ["oplcard.bin", "card16mb.bin", "card32mb.bin", "ps2card.vmc"] {
        let (dirs, files) = compare(&samples.join(name));
        println!("{name}: {dirs} folders, {files} files match");
    }
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with the factory SD's copy and the ps2vmc-tool clone"]
fn ps1_cards_from_real_data() {
    use yaoplm_formats::ps1mc::{Ps1Card, SlotState};
    let sd = testdata()
        .join("backups")
        .join("psxmemcard-gen2-sd-2026-10-26")
        .join("files")
        .join("MemoryCards")
        .join("PS1");
    // The factory's blank cards: every frame free and sound.
    for blank in ["Card1/Card1-1.mcd", "Card4/Card4-1.mcd"] {
        let card = Ps1Card::parse(&std::fs::read(sd.join(blank)).unwrap()).unwrap();
        assert!(card.saves.is_empty(), "{blank}");
        assert!(
            card.slots
                .iter()
                .all(|s| s.state == SlotState::Free && s.checksum_ok),
            "{blank}"
        );
    }
    // FreePSXBoot: read as it is, its odd frames reported, nothing fixed.
    let boot = Ps1Card::parse(&std::fs::read(sd.join("BOOT/BootCard-1.mcd")).unwrap()).unwrap();
    for save in &boot.saves {
        println!(
            "PS1 boot card: {} {:?} {} bytes {:?}",
            save.name, save.blocks, save.size, save.title
        );
    }
    let odd: Vec<_> = boot
        .slots
        .iter()
        .filter(|s| !s.checksum_ok || matches!(s.state, SlotState::Corrupt(_)))
        .collect();
    println!("PS1 boot card: {} odd frames", odd.len());
    // ePSXe's card from ps2vmc-tool's samples.
    let sample = testdata()
        .join("refs")
        .join("bucanero__ps2vmc-tool")
        .join("samples")
        .join("epsxe000.mcr");
    let card = Ps1Card::parse(&std::fs::read(sample).unwrap()).unwrap();
    assert!(!card.saves.is_empty());
    for save in &card.saves {
        assert!(!save.blocks.is_empty() && save.blocks.len() <= 15);
        println!(
            "epsxe000.mcr: {} {:?} {:?}",
            save.name, save.blocks, save.title
        );
    }
}
