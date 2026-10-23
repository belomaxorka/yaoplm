//! The real Redump PS2 DAT, when `YAOPLM_TESTDATA` has one in `dat/`
//! (downloaded by hand, at most once a week: ADR-0007, META-2).

use std::path::PathBuf;
use std::time::Instant;

use yaoplm_hash::dat;
use yaoplm_hash::redump::{RedumpIndex, serial_key};

fn real_dat() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("YAOPLM_TESTDATA")?).join("dat");
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("dat")))
}

#[test]
#[ignore = "needs YAOPLM_TESTDATA with dat/<Redump PS2 DAT>.dat"]
fn reads_the_redump_ps2_dat() {
    let path = real_dat().expect("no DAT in YAOPLM_TESTDATA/dat");
    let bytes = std::fs::read(&path).unwrap();
    let started = Instant::now();
    let datafile = dat::parse(&bytes).unwrap();
    let parsed = started.elapsed();
    assert_eq!(datafile.header.name, "Sony - PlayStation 2");
    assert!(datafile.games.len() > 11_000, "{}", datafile.games.len());
    let roms: usize = datafile.games.iter().map(|g| g.roms.len()).sum();
    let without_sha1 = datafile
        .games
        .iter()
        .flat_map(|g| &g.roms)
        .filter(|r| r.sha1.is_none())
        .count();
    assert_eq!(without_sha1, 0, "every rom has a SHA-1");
    let unparsed: Vec<String> = datafile
        .games
        .iter()
        .filter_map(|g| g.serial.clone())
        .filter(|s| serial_key(s).is_none())
        .collect();
    let index = RedumpIndex::new(datafile);
    println!(
        "{}: {} games, {roms} roms, parsed in {parsed:?}; serials without a key: {unparsed:?}",
        path.display(),
        index.len()
    );
    assert!(index.has_serials());
    assert!(!index.games_for("SLUS_200.62").is_empty());
    assert!(!index.games_for("SLPM_650.02").is_empty());
}
