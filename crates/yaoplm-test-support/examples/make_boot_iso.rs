//! Builds the console test image: an open homebrew ELF booted as a PS2 disc.
//!
//! ```text
//! cargo run -p yaoplm-test-support --example make_boot_iso -- \
//!     --elf <homebrew.elf> --out <image.iso> [--id YAOP_000.01] [--min-mib 4300]
//! ```
//!
//! `--min-mib` pads the image with zero sectors, e.g. past 4 GiB to test
//! `ul` splitting on FAT32. The output file must not exist yet.

use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use yaoplm_test_support::fixtures::boot_iso;

struct Args {
    elf: PathBuf,
    out: PathBuf,
    id: String,
    min_bytes: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut elf = None;
    let mut out = None;
    let mut id = "YAOP_000.01".to_owned();
    let mut min_bytes = 0;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--elf" => elf = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--id" => id = value()?,
            "--min-mib" => {
                let mib: u64 = value()?.parse().map_err(|_| "--min-mib takes a number")?;
                min_bytes = mib << 20;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let valid_id = id.len() == 11 && id.as_bytes()[4] == b'_' && id.as_bytes()[8] == b'.';
    if !valid_id {
        return Err(format!("--id must look like SLUS_200.62, got {id}"));
    }
    Ok(Args {
        elf: elf.ok_or("--elf is required")?,
        out: out.ok_or("--out is required")?,
        id,
        min_bytes,
    })
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let elf = std::fs::read(&args.elf).map_err(|e| format!("{}: {e}", args.elf.display()))?;
    if !elf.starts_with(b"\x7FELF") {
        return Err(format!("{} is not an ELF file", args.elf.display()));
    }
    let iso = boot_iso(elf, &args.id, args.min_bytes).build();
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.out)
        .map_err(|e| format!("{}: {e}", args.out.display()))?;
    let mut writer = BufWriter::new(file);
    let failed = |e: std::io::Error| format!("writing {}: {e}", args.out.display());
    let written = iso.write_to(&mut writer).map_err(failed)?;
    let file = writer.into_inner().map_err(|e| failed(e.into_error()))?;
    file.sync_all().map_err(failed)?;
    println!(
        "{}: {written} bytes, game ID {}",
        args.out.display(),
        args.id
    );
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("make_boot_iso: {err}");
            ExitCode::FAILURE
        }
    }
}
