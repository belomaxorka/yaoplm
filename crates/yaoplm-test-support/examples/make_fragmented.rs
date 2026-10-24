//! Copies an image onto a FAT32 or exFAT device in a chosen number of
//! pieces, for console checks of OPL's fragment limits (IMP-7b): the copy
//! and a filler file next to it grow in turns, then the filler is deleted.
//!
//! ```text
//! cargo run -p yaoplm-test-support --example make_fragmented -- \
//!     --image <image.iso> --out <device>\CD\<name>.iso --pieces 70
//! ```
//!
//! Writes only `--out` and `<out>.filler`, both of which must not exist yet,
//! and prints how many pieces the OS reports for the copy. Never run it on
//! a device without its owner's go-ahead (CLAUDE.md rule 1).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yaoplm_device::{DeviceFs, HostFs, RelPath};

struct Args {
    image: PathBuf,
    out: PathBuf,
    pieces: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut image = None;
    let mut out = None;
    let mut pieces = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--image" => image = Some(PathBuf::from(value()?)),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--pieces" => {
                let n: u64 = value()?.parse().map_err(|_| "--pieces takes a number")?;
                pieces = Some(n.max(1));
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Args {
        image: image.ok_or("--image is required")?,
        out: out.ok_or("--out is required")?,
        pieces: pieces.ok_or("--pieces is required")?,
    })
}

fn create(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let dir = args.out.parent().ok_or("--out needs a folder")?;
    let name = args
        .out
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("--out needs a file name")?;
    let fs = HostFs::new(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let cluster = u64::from(fs.info().map_err(|e| e.to_string())?.cluster_size);
    let mut image =
        File::open(&args.image).map_err(|e| format!("{}: {e}", args.image.display()))?;
    let len = image.metadata().map_err(|e| e.to_string())?.len();
    // Whole clusters per piece, so every piece starts a new run.
    let piece = len.div_ceil(args.pieces).div_ceil(cluster).max(1) * cluster;

    let filler_path = PathBuf::from(format!("{}.filler", args.out.display()));
    let mut out = create(&args.out)?;
    let mut filler = create(&filler_path)?;
    let gap = vec![0u8; cluster as usize];
    let mut buffer = vec![0u8; piece as usize];
    let mut left = len;
    let failed = |e: std::io::Error| format!("writing {}: {e}", args.out.display());
    while left > 0 {
        let n = piece.min(left) as usize;
        image
            .read_exact(&mut buffer[..n])
            .map_err(|e| e.to_string())?;
        out.write_all(&buffer[..n]).map_err(failed)?;
        out.sync_data().map_err(failed)?;
        left -= n as u64;
        if left > 0 {
            filler.write_all(&gap).map_err(failed)?;
            filler.sync_data().map_err(failed)?;
        }
    }
    out.sync_all().map_err(failed)?;
    drop((out, filler));
    std::fs::remove_file(&filler_path).map_err(|e| format!("{}: {e}", filler_path.display()))?;

    let runs = RelPath::new(name)
        .map_err(|e| e.to_string())
        .and_then(|rel| fs.fragments(&rel).map_err(|e| e.to_string()))?;
    match runs {
        Some(runs) => println!("{}: {len} bytes in {runs} pieces", args.out.display()),
        None => println!(
            "{}: {len} bytes; the OS does not say how many pieces",
            args.out.display()
        ),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("make_fragmented: {err}");
            ExitCode::FAILURE
        }
    }
}
