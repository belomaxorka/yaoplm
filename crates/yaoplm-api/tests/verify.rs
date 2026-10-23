//! Dump verification through `dispatch` (GAME-5, META-2): a DAT imported
//! from a file, VerifyGames as a job, verdicts per game, the hash cache.
//! No network: the DAT is a generated file.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;

use serde_json::{Value, json};
use yaoplm_api::jobs::{JobInfo, JobLogEntry, JobResult, JobStatus};
use yaoplm_api::verify::{GameVerification, UnknownReason, VerificationReport, VerifyStatus};
use yaoplm_api::{Api, JobId};
use yaoplm_formats::Media;
use yaoplm_formats::ul_cfg::UlRecord;
use yaoplm_formats::zso;
use yaoplm_test_support::dat::DatBuilder;
use yaoplm_test_support::{Variant, fixtures};

struct Setup {
    _dir: tempfile::TempDir,
    api: Api,
    device: String,
    dat: String,
}

/// Five games, each with another verdict once the DAT is in:
/// - `CD/SLUS_200.62.Retail.iso`: in the DAT byte for byte (verified);
/// - `DVD/SLUS_212.18.Dvd9.zso`: a ZSO of an ISO the DAT lists (verified);
/// - `CD/SCES_500.03.Lf Game.iso`: the DAT lists SCES-50003 with other
///   bytes of the same size (bad dump);
/// - `CD/SLES_123.45.Cd Game.iso`: the DAT has it as BIN/CUE (unknown);
/// - a `ul` game the DAT does not list (unknown).
fn setup() -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    for folder in ["CD", "DVD"] {
        fs::create_dir_all(device.join(folder)).unwrap();
    }
    let retail = Variant::Retail.build().to_vec();
    fs::write(device.join("CD/SLUS_200.62.Retail.iso"), &retail).unwrap();

    let dvd9 = Variant::Dvd9.build().to_vec();
    let mut packed = Cursor::new(Vec::new());
    zso::compress(
        &mut Cursor::new(&dvd9),
        dvd9.len() as u64,
        &mut packed,
        &mut |_| {},
    )
    .unwrap();
    fs::write(device.join("DVD/SLUS_212.18.Dvd9.zso"), packed.into_inner()).unwrap();

    let lf = Variant::LfOnly.build().to_vec();
    fs::write(device.join("CD/SCES_500.03.Lf Game.iso"), &lf).unwrap();
    let mut damaged = lf.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 0xFF;

    let cd = fixtures::boot_iso(fixtures::placeholder_elf(), "SLES_123.45", 0)
        .build()
        .to_vec();
    fs::write(device.join("CD/SLES_123.45.Cd Game.iso"), &cd).unwrap();
    let bin: Vec<u8> = cd
        .chunks(2048)
        .flat_map(|sector| {
            let mut raw = vec![0u8; 24];
            raw.extend_from_slice(sector);
            raw.resize(2352, 0);
            raw
        })
        .collect();

    let ul = fixtures::boot_iso(fixtures::placeholder_elf(), "SLPM_999.99", 0)
        .build()
        .to_vec();
    let record = UlRecord::new("Ul Game", "SLPM_999.99", 1, Media::Dvd).unwrap();
    fs::write(device.join(record.chunk_name(0)), &ul).unwrap();
    fs::write(device.join("ul.cfg"), record.as_bytes()).unwrap();

    let dat = DatBuilder::new()
        .iso("Retail (USA)", Some("SLUS-20062"), &retail)
        .iso("Dvd9 (USA)", Some("SLUS 21218"), &dvd9)
        .iso(
            "Lf Game (Europe)",
            Some("SCES-50003, SCES-50003-P"),
            &damaged,
        )
        .bin_cue("Cd Game (Europe)", Some("SLES 12345"), &bin)
        .build();
    let dat_path = dir.path().join("Sony - PlayStation 2 - Datfile (4).dat");
    fs::write(&dat_path, dat).unwrap();

    Setup {
        api: Api::new().with_data_dir(dir.path().join("data")),
        device: device.to_string_lossy().into_owned(),
        dat: dat_path.to_string_lossy().into_owned(),
        _dir: dir,
    }
}

fn call(api: &Api, request: Value) -> Value {
    serde_json::to_value(api.dispatch_json(&request.to_string()).unwrap()).unwrap()
}

fn run(api: &Api, request: Value) -> JobInfo {
    let started = call(api, request);
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    info
}

fn verify(api: &Api, request: Value) -> (VerificationReport, JobInfo) {
    let info = run(api, request);
    match info.result.clone() {
        Some(JobResult::Verification(report)) => (*report, info),
        other => panic!("unexpected result {other:?}"),
    }
}

fn game<'a>(report: &'a VerificationReport, startup: &str) -> &'a GameVerification {
    report
        .games
        .iter()
        .find(|g| g.startup == startup)
        .unwrap_or_else(|| panic!("{startup} not verified"))
}

#[test]
fn verdicts_per_game_with_an_imported_dat() {
    let s = setup();
    assert_eq!(
        call(&s.api, json!({"type": "RedumpStatus"})),
        json!({"type": "Redump", "dat": null})
    );

    // Without a DAT the games are hashed, and nothing can be said.
    let (report, info) = verify(&s.api, json!({"type": "VerifyGames", "root": s.device}));
    assert_eq!(report.games.len(), 5);
    assert!(report.dat.is_none());
    for g in &report.games {
        assert_eq!(g.status, VerifyStatus::Unknown, "{g:?}");
        assert_eq!(g.reason, Some(UnknownReason::NoDat));
        assert!(g.hashes.is_some() && !g.cached);
    }
    let logged = info
        .log
        .iter()
        .filter(|e| matches!(e, JobLogEntry::Verify { .. }))
        .count();
    assert_eq!(logged, 5);

    let imported = run(&s.api, json!({"type": "UpdateRedump", "file": s.dat}));
    let Some(JobResult::Redump(result)) = imported.result else {
        panic!("unexpected result {:?}", imported.result);
    };
    assert_eq!(
        serde_json::to_value(result.update).unwrap(),
        json!("imported")
    );
    assert_eq!(result.dat.games, 4);
    assert!(result.dat.serials);
    let status = call(&s.api, json!({"type": "RedumpStatus"}));
    assert_eq!(status["dat"]["games"], 4);

    // Every image comes from the hash cache now.
    let (report, _) = verify(&s.api, json!({"type": "VerifyGames", "root": s.device}));
    assert!(report.games.iter().all(|g| g.cached), "{report:?}");
    assert_eq!(report.dat.as_ref().map(|d| d.games), Some(4));

    let retail = game(&report, "SLUS_200.62");
    assert_eq!(retail.status, VerifyStatus::Verified);
    let entry = retail.redump.as_ref().unwrap();
    assert_eq!(entry.name, "Retail (USA)");
    assert_eq!(entry.url.as_deref(), Some("https://redump.info/disc/1000"));
    assert_eq!(retail.serial_matches, Some(true));

    let zso = game(&report, "SLUS_212.18");
    assert_eq!(
        zso.status,
        VerifyStatus::Verified,
        "the ZSO hashes as its ISO"
    );
    assert_eq!(zso.redump.as_ref().unwrap().name, "Dvd9 (USA)");

    let bad = game(&report, "SCES_500.03");
    assert_eq!(bad.status, VerifyStatus::BadDump);
    assert_eq!(bad.candidates.len(), 1);
    assert!(bad.candidates[0].same_size);

    let cd = game(&report, "SLES_123.45");
    assert_eq!(cd.status, VerifyStatus::Unknown);
    assert_eq!(cd.reason, Some(UnknownReason::CdImage));
    assert!(
        cd.candidates[0].same_size,
        "same number of sectors as the BIN"
    );

    let ul = game(&report, "SLPM_999.99");
    assert_eq!(ul.status, VerifyStatus::Unknown);
    assert_eq!(ul.reason, Some(UnknownReason::NotListed));
    assert_eq!(ul.ul_index, Some(0));

    // A scan shows the verdicts from the cache without reading the images.
    let scan = run(&s.api, json!({"type": "ScanLibrary", "root": s.device}));
    let Some(JobResult::Library(library)) = scan.result else {
        panic!("unexpected result {:?}", scan.result);
    };
    let dump = |startup: &str| {
        library
            .games
            .iter()
            .find(|g| g.startup == startup)
            .and_then(|g| g.dump.clone())
            .unwrap()
    };
    assert_eq!(dump("SLUS_200.62").status, VerifyStatus::Verified);
    assert_eq!(dump("SLUS_200.62").redump.as_deref(), Some("Retail (USA)"));
    assert_eq!(dump("SCES_500.03").status, VerifyStatus::BadDump);
    // A changed image loses its verdict.
    let retail = std::path::Path::new(&s.device).join("CD/SLUS_200.62.Retail.iso");
    let mut bytes = fs::read(&retail).unwrap();
    bytes.extend_from_slice(&[0u8; 2048]);
    fs::write(&retail, bytes).unwrap();
    let scan = run(&s.api, json!({"type": "ScanLibrary", "root": s.device}));
    let Some(JobResult::Library(library)) = scan.result else {
        panic!("unexpected result {:?}", scan.result);
    };
    let changed = library.games.iter().find(|g| g.startup == "SLUS_200.62");
    assert_eq!(changed.unwrap().dump, None);
}

#[test]
fn one_game_and_md5() {
    let s = setup();
    run(&s.api, json!({"type": "UpdateRedump", "file": s.dat}));
    let one = json!({
        "type": "VerifyGames",
        "root": s.device,
        "games": [{"kind": "image", "path": "CD/SLUS_200.62.Retail.iso"}],
    });
    let (report, _) = verify(&s.api, one.clone());
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].status, VerifyStatus::Verified);
    assert!(report.games[0].hashes.as_ref().unwrap().md5.is_none());

    // MD5 was not computed yet: the image is read again.
    let mut with_md5 = one.clone();
    with_md5["md5"] = json!(true);
    let (report, _) = verify(&s.api, with_md5.clone());
    assert!(!report.games[0].cached);
    let md5 = report.games[0].hashes.as_ref().unwrap().md5.clone();
    assert_eq!(md5.as_deref().map(str::len), Some(32));
    let (report, _) = verify(&s.api, with_md5);
    assert!(report.games[0].cached);

    let ul = json!({
        "type": "VerifyGames",
        "root": s.device,
        "games": [{"kind": "ul", "index": 0, "startup": "SLPM_999.99", "name": "Ul Game"}],
    });
    let (report, _) = verify(&s.api, ul);
    assert_eq!(report.games.len(), 1);
    assert_eq!(report.games[0].startup, "SLPM_999.99");
}

#[test]
fn junk_dat_is_refused() {
    let s = setup();
    let junk = std::path::Path::new(&s.dat).with_file_name("junk.dat");
    fs::write(&junk, "<html><body>not a DAT</body></html>").unwrap();
    let started = call(
        &s.api,
        json!({"type": "UpdateRedump", "file": junk.to_string_lossy()}),
    );
    let job: JobId = serde_json::from_value(started["job"].clone()).unwrap();
    let info = s.api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Failed);
    assert_eq!(
        call(&s.api, json!({"type": "RedumpStatus"}))["dat"],
        Value::Null
    );
}
