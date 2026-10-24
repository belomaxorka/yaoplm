//! Version strings of real loader ELFs (LDR-4, research/16 §1.6). Needs
//! `YAOPLM_TESTDATA` with `downloads/loaders/`: official release assets of
//! OPL, its forks, NHDDL and Neutrino, unpacked (never committed), and
//! `downloads/loaders/elf-versions.tsv` with `<path>\t<expected version or ->`
//! for every ELF there, the versions taken from the release and file names.
//!
//! Every ELF is also read damaged: cut short, with header fields and stream
//! bytes changed. A damaged file may lose its version but never gains
//! another one, and nothing panics. Run with
//! `cargo test -p yaoplm-formats --test loader_elfs -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use yaoplm_formats::ps2_elf;

fn corpus() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("YAOPLM_TESTDATA")?).join("downloads/loaders");
    root.is_dir().then_some(root)
}

fn elfs(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            elfs(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("elf"))
        {
            out.push(path);
        }
    }
}

fn key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn version(data: &[u8]) -> Option<String> {
    ps2_elf::read_version(data)
        .ok()
        .flatten()
        .map(|v| v.version)
}

/// Copies of `data` damaged in the places the reader trusts.
fn damaged(data: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for part in [4, 2] {
        out.push((
            format!("cut to 1/{part}"),
            data[..data.len() / part].to_vec(),
        ));
    }
    out.push(("cut by one byte".into(), data[..data.len() - 1].to_vec()));
    let u32_at = |at: usize| {
        data.get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap())) as usize
    };
    let phoff = u32_at(0x1C);
    let start = u32_at(phoff + 4).min(data.len());
    let mut set = |what: &str, at: usize, bytes: &[u8]| {
        if at + bytes.len() <= data.len() {
            let mut copy = data.to_vec();
            copy[at..at + bytes.len()].copy_from_slice(bytes);
            out.push((what.to_owned(), copy));
        }
    };
    set("magic", 1, b"X");
    set("class", 4, &[2]);
    set("machine", 0x12, &[3]);
    set(
        "program headers past the end",
        0x1C,
        &u32::MAX.to_le_bytes(),
    );
    set("segment past the end", phoff + 4, &u32::MAX.to_le_bytes());
    set("huge unpacked size", start + 8, &u32::MAX.to_le_bytes());
    set("bad LZMA properties", start + 24, &[0xFF]);
    set("huge dictionary", start + 25, &u32::MAX.to_le_bytes());
    let stream = start + 29;
    if stream < data.len() {
        for i in 1..=6 {
            let at = stream + (data.len() - stream) * i / 7;
            let mut copy = data.to_vec();
            copy[at] ^= 0xFF;
            out.push((format!("stream byte {i}/7 flipped"), copy));
        }
    }
    out
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA/downloads/loaders"]
fn real_loader_elfs() {
    let Some(root) = corpus() else {
        panic!("set YAOPLM_TESTDATA to a folder with downloads/loaders/");
    };
    let expected: BTreeMap<String, String> = std::fs::read_to_string(root.join("elf-versions.tsv"))
        .expect("downloads/loaders/elf-versions.tsv")
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once('\t'))
        .map(|(p, v)| (p.to_owned(), v.trim().to_owned()))
        .collect();
    let mut files = Vec::new();
    elfs(&root, &mut files);
    files.sort();
    assert!(!files.is_empty());

    let started = Instant::now();
    let mut wrong = Vec::new();
    let (mut packed, mut plain, mut none) = (0, 0, 0);
    // One damaged set per release folder keeps the run short.
    let mut damaged_folders = std::collections::BTreeSet::new();
    let mut damaged_runs = 0;
    for path in &files {
        let key = key(&root, path);
        let data = std::fs::read(path).unwrap();
        let got = ps2_elf::read_version(&data);
        let found = got.as_ref().ok().cloned().flatten();
        match &found {
            Some(v) if v.packed => packed += 1,
            Some(_) => plain += 1,
            None => none += 1,
        }
        let shown = found.as_ref().map_or("-".to_owned(), |v| v.version.clone());
        println!(
            "{:<6} {:<45} {key}",
            match (&got, &found) {
                (Err(_), _) => "no-elf",
                (_, Some(v)) if v.packed => "packed",
                (_, Some(_)) => "plain",
                _ => "-",
            },
            shown
        );
        // `v1.2.0-Beta-4257*`: release names give only the start of some
        // versions (a fork's flavour and commit follow).
        let matches = |want: &str| match want.strip_suffix('*') {
            Some(prefix) => shown.starts_with(prefix),
            None => want == shown,
        };
        match expected.get(&key) {
            Some(want) if matches(want) => {}
            Some(want) => wrong.push(format!("{key}: expected {want}, read {shown}")),
            None => wrong.push(format!("{key}: not in elf-versions.tsv (read {shown})")),
        }

        let folder = key.split('/').take(3).collect::<Vec<_>>().join("/");
        if data.len() > 0x100 && damaged_folders.insert(folder) {
            for (what, copy) in damaged(&data) {
                damaged_runs += 1;
                let read = version(&copy);
                if read.is_some() && read.as_deref() != found.as_ref().map(|v| v.version.as_str()) {
                    wrong.push(format!(
                        "{key} ({what}): read {read:?}, the intact file says {shown}"
                    ));
                }
            }
        }
    }
    println!(
        "{} ELFs: {packed} packed, {plain} plain, {none} without a version; {damaged_runs} damaged copies; {:.1} s",
        files.len(),
        started.elapsed().as_secs_f64()
    );
    assert!(
        wrong.is_empty(),
        "{} problems:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
