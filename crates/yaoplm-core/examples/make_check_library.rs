//! Builds a small OPL library for checking `yaoplm scan` against what OPL
//! lists on the console (ROADMAP M1 "Готово, когда").
//!
//! ```text
//! cargo run -p yaoplm-core --example make_check_library -- \
//!     --elf <homebrew.elf> --out <empty folder>
//! ```
//!
//! Every entry exercises one rule: old and new file names, ZSO, a `ul` game
//! (its chunk name proves `USBA_crc32` on real hardware), images OPL hides
//! and images with odd IDs. Nothing here crashes OPL. `EXPECTED.txt` lists
//! what OPL 1.2.0 beta should show.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yaoplm_formats::Media;
use yaoplm_formats::ul_cfg::{UlCfg, UlRecord};
use yaoplm_formats::zso;
use yaoplm_test_support::{Variant, fixtures};

const EXPECTED: &str = "\
What OPL 1.2.0 beta should show for this library
================================================

Bootable (wLaunchELF must start; each has its own game ID):
  CD   YAOP_000.01  wLaunchELF Old Name   old-style name, ID taken from the file name
  CD   YAOP_000.02  wLaunchELF New Name   new-style name, ID read from SYSTEM.CNF
  CD   YAOP_000.03  wLaunchELF ZSO        ZSO image (OPL 1.1.0 does not list it)
  CD   YAOP_000.04  wLaunchELF UL         ul.cfg game; the chunk name tests USBA_crc32

Listed with odd IDs (placeholder ELF inside, do NOT launch):
  DVD  DATA\\MAIN.E  Odd Subfolder ELF    BOOT2 points into a subfolder
  DVD  SLPM_123.45  Odd Long ID          ID cut to 11 characters
  DVD  SLUS_212.18  Odd DVD-9            second volume descriptor (layer break)

Not listed at all:
  DVD  Hidden No BOOT2.iso, Hidden PS1.iso, Hidden Lowercase Key.iso,
       Hidden No Version.iso, Hidden Lowercase Name.iso
  DVD  Not An Image.cso (OPL does not read .cso)

Compare with: yaoplm scan <this folder or the stick>
";

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let mut elf = None;
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--elf" => elf = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok((
        elf.ok_or("--elf is required")?,
        out.ok_or("--out is required")?,
    ))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn boot_iso(elf: &[u8], startup: &str) -> Vec<u8> {
    fixtures::boot_iso(elf.to_vec(), startup, 0)
        .build()
        .to_vec()
}

fn run() -> Result<(), String> {
    let (elf_path, out) = parse_args()?;
    let elf = fs::read(&elf_path).map_err(|e| format!("{}: {e}", elf_path.display()))?;
    if !elf.starts_with(b"\x7FELF") {
        return Err(format!("{} is not an ELF file", elf_path.display()));
    }
    let not_empty = fs::read_dir(&out).is_ok_and(|mut entries| entries.next().is_some());
    if not_empty {
        return Err(format!("{} is not empty", out.display()));
    }

    write(
        &out.join("CD/YAOP_000.01.wLaunchELF Old Name.iso"),
        &boot_iso(&elf, "YAOP_000.01"),
    )?;
    write(
        &out.join("CD/wLaunchELF New Name.iso"),
        &boot_iso(&elf, "YAOP_000.02"),
    )?;

    let iso = boot_iso(&elf, "YAOP_000.03");
    let mut compressed = Cursor::new(Vec::new());
    zso::compress(
        &mut Cursor::new(&iso),
        iso.len() as u64,
        &mut compressed,
        &mut |_| {},
    )
    .map_err(|e| e.to_string())?;
    write(&out.join("CD/wLaunchELF ZSO.zso"), compressed.get_ref())?;

    let record =
        UlRecord::new("wLaunchELF UL", "YAOP_000.04", 1, Media::Cd).map_err(|e| e.to_string())?;
    write(
        &out.join(record.chunk_name(0)),
        &boot_iso(&elf, "YAOP_000.04"),
    )?;
    let table = UlCfg {
        records: vec![record],
        ..UlCfg::default()
    };
    write(&out.join("ul.cfg"), &table.to_bytes())?;

    for (name, variant) in [
        ("Odd Subfolder ELF", Variant::SubfolderElf),
        ("Odd Long ID", Variant::LongId),
        ("Odd DVD-9", Variant::Dvd9),
        ("Hidden No BOOT2", Variant::NoBoot2),
        ("Hidden PS1", Variant::Ps1),
        ("Hidden Lowercase Key", Variant::LowercaseKey),
        ("Hidden No Version", Variant::NoVersionSuffix),
        ("Hidden Lowercase Name", Variant::LowercaseName),
    ] {
        write(
            &out.join(format!("DVD/{name}.iso")),
            &variant.build().to_vec(),
        )?;
    }
    write(&out.join("DVD/Not An Image.cso"), b"CISO")?;
    for folder in ["CFG", "THM", "LNG", "ART", "VMC", "CHT", "APPS"] {
        fs::create_dir_all(out.join(folder)).map_err(|e| e.to_string())?;
    }
    write(
        &out.join("EXPECTED.txt"),
        EXPECTED.replace('\n', "\r\n").as_bytes(),
    )?;
    println!("check library written to {}", out.display());
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("make_check_library: {err}");
            ExitCode::FAILURE
        }
    }
}
