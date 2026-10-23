//! Packs a small test image the ways people download games (IMP-1b), for
//! the console checklist: a deflated zip and a 7z with the ISO in a folder,
//! a DVD CHD, a CD CHD with an audio track, and a Redump-style BIN/CUE with
//! one file per track.
//!
//! ```text
//! cargo run -p yaoplm-test-support --example pack -- <image.iso> <out-dir>
//! ```
//!
//! Everything is built in memory: meant for images of a few megabytes, such
//! as the wLaunchELF disc from `make_boot_iso`. The output folder must not
//! exist yet.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yaoplm_test_support::bincue::{RawMode, to_bin};
use yaoplm_test_support::packed::{chd_cd, chd_dvd, seven_zip, zip};

/// Silent CD audio after the data: two seconds.
const AUDIO_FRAMES: usize = 150;

fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let path = dir.join(name);
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("{} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

fn run(image: &Path, out: &Path) -> Result<(), String> {
    let iso = std::fs::read(image).map_err(|e| format!("{}: {e}", image.display()))?;
    if iso.is_empty() || iso.len() % 2048 != 0 {
        return Err(format!(
            "{} is not a whole number of 2048-byte sectors",
            image.display()
        ));
    }
    let stem = image
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or("the image has no file name")?;
    if out.exists() {
        return Err(format!("{} exists already", out.display()));
    }
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let inner = format!("{stem}/{stem}.iso");
    let err = |e: std::io::Error| e.to_string();

    write(
        out,
        &format!("{stem}.zip"),
        &zip(&[(&inner, &iso)], false).map_err(err)?,
    )?;
    write(
        out,
        &format!("{stem}.7z"),
        &seven_zip(&[(&inner, &iso)]).map_err(err)?,
    )?;
    write(
        out,
        &format!("{stem} (DVD).chd"),
        &chd_dvd(&iso).map_err(err)?,
    )?;
    write(
        out,
        &format!("{stem} (CD).chd"),
        &chd_cd(&iso, RawMode::Mode2_2352, AUDIO_FRAMES).map_err(err)?,
    )?;

    let track1 = format!("{stem} (Track 1).bin");
    let track2 = format!("{stem} (Track 2).bin");
    write(out, &track1, &to_bin(&iso, RawMode::Mode2_2352, 0))?;
    write(out, &track2, &vec![0u8; 2352 * AUDIO_FRAMES])?;
    let sheet = format!(
        "FILE \"{track1}\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\nFILE \"{track2}\" BINARY\r\n  TRACK 02 AUDIO\r\n    INDEX 00 00:00:00\r\n    INDEX 01 00:02:00\r\n"
    );
    write(out, &format!("{stem}.cue"), sheet.as_bytes())
}

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [image, out] = args.as_slice() else {
        eprintln!("usage: pack <image.iso> <out-dir>");
        return ExitCode::from(2);
    };
    match run(image, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}
