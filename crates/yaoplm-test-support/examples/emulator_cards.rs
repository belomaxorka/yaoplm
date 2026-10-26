//! Writes the fixture's boot cards for a look with other tools:
//! `cargo run -p yaoplm-test-support --example emulator_cards -- <dir>`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("a folder"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("fmcb.mcd"),
        yaoplm_test_support::emulator::fmcb_card(),
    )
    .unwrap();
    std::fs::write(
        dir.join("ps2bbl.mcd"),
        yaoplm_test_support::emulator::ps2bbl_card(),
    )
    .unwrap();
}
