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
    let dat = result.dat.unwrap();
    assert_eq!(dat.games, 4);
    assert!(dat.serials);
    // Imported, never asked: Redump may be asked right away.
    assert_eq!(dat.next_check, None);
    assert_eq!(result.next_check, None);
    assert!(!dat.source.starts_with(r"\\?\"), "{}", dat.source);
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

/// Games added with `verify` (GAME-5): hashed while written, judged at
/// once, and the hashes cached under the new files, so later checks and
/// scans read nothing. A CD from its raw BIN is judged by the track.
#[test]
fn checks_while_adding_games() {
    use yaoplm_test_support::bincue::{RawMode, to_bin};

    let dir = tempfile::tempdir().unwrap();
    let device = dir.path().join("stick");
    let sources = dir.path().join("sources");
    fs::create_dir_all(&device).unwrap();
    fs::create_dir_all(&sources).unwrap();
    let dvd = Variant::Dvd9.build().to_vec();
    fs::write(sources.join("Dvd Game.iso"), &dvd).unwrap();
    let cd = Variant::Retail.build().to_vec();
    let bin = to_bin(&cd, RawMode::Mode2_2352, 0);
    fs::write(sources.join("Cd Game.bin"), &bin).unwrap();
    fs::write(
        sources.join("Cd Game.cue"),
        "FILE \"Cd Game.bin\" BINARY\r\n  TRACK 01 MODE2/2352\r\n    INDEX 01 00:00:00\r\n",
    )
    .unwrap();
    let unknown = fixtures::boot_iso(fixtures::placeholder_elf(), "SLPM_999.99", 0)
        .build()
        .to_vec();
    fs::write(sources.join("Homebrew.iso"), &unknown).unwrap();
    let dat = DatBuilder::new()
        .iso("Dvd9 (USA)", Some("SLUS 21218"), &dvd)
        .bin_cue("Retail (USA)", Some("SLUS-20062"), &bin)
        .build();
    let dat_path = dir.path().join("redump.dat");
    fs::write(&dat_path, dat).unwrap();
    let api = Api::new().with_data_dir(dir.path().join("data"));
    run(&api, json!({"type": "UpdateRedump", "file": dat_path}));

    let plan = call(
        &api,
        json!({
            "type": "PlanImport",
            "root": device,
            "sources": [sources],
            "verify": true,
        }),
    );
    assert_eq!(plan["executable"], true, "{plan}");
    let info = run(
        &api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": false}),
    );
    let Some(JobResult::Execution(report)) = info.result.clone() else {
        panic!("unexpected result {:?}", info.result);
    };
    assert!(report.error.is_none(), "{report:?}");
    assert_eq!(report.checks.len(), 3, "{report:?}");
    let check = |startup: &str| {
        report
            .checks
            .iter()
            .find(|g| g.startup == startup)
            .unwrap_or_else(|| panic!("{startup} not checked"))
    };
    assert_eq!(check("SLUS_212.18").status, VerifyStatus::Verified);
    assert!(check("SLUS_212.18").track.is_none());
    let cd_check = check("SLUS_200.62");
    assert_eq!(cd_check.status, VerifyStatus::Verified, "{cd_check:?}");
    assert_eq!(
        cd_check.redump.as_ref().unwrap().rom,
        "Retail (USA).bin",
        "matched through the data track"
    );
    assert_eq!(cd_check.track.as_ref().unwrap().size, bin.len() as u64);
    assert_eq!(cd_check.hashes.as_ref().unwrap().size, cd.len() as u64);
    assert_eq!(check("SLPM_999.99").reason, Some(UnknownReason::NotListed));
    // One log line per game, after its transaction.
    let verdicts = info
        .log
        .iter()
        .filter(|e| matches!(e, JobLogEntry::Verify { cached: false, .. }))
        .count();
    assert_eq!(verdicts, 3);

    // The cache already has every image under its new file.
    let (verified, _) = verify(&api, json!({"type": "VerifyGames", "root": device}));
    assert_eq!(verified.games.len(), 3);
    assert!(verified.games.iter().all(|g| g.cached), "{verified:?}");
    let cd_game = game(&verified, "SLUS_200.62");
    assert_eq!(cd_game.status, VerifyStatus::Verified);
    assert!(cd_game.track.is_some());

    // Read again: the ISO hashes lead back to the track by their content.
    let (fresh, _) = verify(
        &api,
        json!({"type": "VerifyGames", "root": device, "fresh": true}),
    );
    let cd_game = game(&fresh, "SLUS_200.62");
    assert!(!cd_game.cached);
    assert_eq!(cd_game.status, VerifyStatus::Verified);

    let scan = run(&api, json!({"type": "ScanLibrary", "root": device}));
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
    assert!(dump("SLUS_200.62").by_track);
    assert!(!dump("SLUS_212.18").by_track);

    // Without `verify` nothing is hashed.
    let device2 = dir.path().join("stick2");
    fs::create_dir_all(&device2).unwrap();
    let plan = call(
        &api,
        json!({"type": "PlanImport", "root": device2, "sources": [sources]}),
    );
    let info = run(
        &api,
        json!({"type": "ExecutePlan", "plan": plan["id"], "confirmDelete": false}),
    );
    let Some(JobResult::Execution(report)) = info.result else {
        panic!("unexpected result");
    };
    assert!(report.checks.is_empty());
}
