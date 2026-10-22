//! Writes every fixture variant as `<label>.iso` into a folder, e.g. as a
//! seed corpus for `cargo fuzz run iso`:
//!
//! ```text
//! cargo run -p yaoplm-test-support --example write_fixtures -- fuzz/corpus/iso
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use yaoplm_test_support::Variant;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: write_fixtures <folder>");
        return ExitCode::FAILURE;
    };
    if let Err(err) = std::fs::create_dir_all(&dir) {
        eprintln!("{}: {err}", dir.display());
        return ExitCode::FAILURE;
    }
    for variant in Variant::ALL {
        let path = dir.join(format!("{}.iso", variant.label()));
        if let Err(err) = std::fs::write(&path, variant.build().to_vec()) {
            eprintln!("{}: {err}", path.display());
            return ExitCode::FAILURE;
        }
    }
    println!("{} images in {}", Variant::ALL.len(), dir.display());
    ExitCode::SUCCESS
}
