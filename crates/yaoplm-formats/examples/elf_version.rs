//! Prints what `ps2_elf` reads from loader ELFs: packed or not, the size of
//! the program and its version string (research/16 §1.6).
//! `cargo run -p yaoplm-formats --example elf_version -- <file.elf>...`

use yaoplm_formats::ps2_elf;

fn main() {
    for path in std::env::args().skip(1) {
        let data = match std::fs::read(&path) {
            Ok(data) => data,
            Err(e) => {
                println!("{path}: {e}");
                continue;
            }
        };
        match ps2_elf::program(&data) {
            Ok(program) => println!(
                "{path}: {} program of {} bytes, version {}",
                if program.packed { "packed" } else { "plain" },
                program.bytes.len(),
                ps2_elf::find_version(&program.bytes).unwrap_or_else(|| "-".into())
            ),
            Err(e) => println!("{path}: {e}"),
        }
    }
}
