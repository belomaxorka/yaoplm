//! Builds the fixture library the Playwright tests run against
//! (TESTING.md, "Проверка интерфейса агентом"):
//!
//! ```text
//! cargo run -p yaoplm-api --example make_library -- <folder> [--games N]
//! ```
//!
//! `<folder>/library` is a device root with games in every format and a
//! few things to warn about; `<folder>/sources` holds images to import.
//! Pass `--games N` to add N more games for virtualization checks. Nothing
//! here is a real game: every image comes from `yaoplm-test-support`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use yaoplm_api::jobs::{JobResult, JobStatus};
use yaoplm_api::{Api, Layout, Request, Response};
use yaoplm_test_support::Variant;
use yaoplm_test_support::bincue::{RawMode, cue_sheet, to_bin};

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// Imports `sources` into `root` through the API, like the UI does.
fn import(root: &Path, sources: &[PathBuf], layout: Layout) {
    let api = Api::new();
    let Response::Plan(plan) = api
        .dispatch(Request::PlanImport {
            root: root.to_string_lossy().into_owned(),
            sources: sources
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            profile: None,
            name_style: None,
            transfer: None,
            layout: Some(layout),
            overrides: None,
        })
        .unwrap()
    else {
        panic!("PlanImport answers with a plan");
    };
    assert!(plan.executable, "{plan:#?}");
    let Response::Job { job } = api
        .dispatch(Request::ExecutePlan {
            plan: plan.id,
            confirm_delete: None,
        })
        .unwrap()
    else {
        panic!("ExecutePlan answers with a job");
    };
    let info = api.jobs().wait(job).unwrap();
    assert_eq!(info.status, JobStatus::Finished, "{info:?}");
    match info.result {
        Some(JobResult::Execution(report)) => assert!(report.error.is_none(), "{report:?}"),
        other => panic!("unexpected result {other:?}"),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(dir) = args.first().map(PathBuf::from) else {
        eprintln!("usage: make_library <folder> [--games N]");
        return ExitCode::FAILURE;
    };
    let extra: usize = args
        .iter()
        .position(|a| a == "--games")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);

    let library = dir.join("library");
    let sources = dir.join("sources");
    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&sources);
    for folder in ["CD", "DVD", "CFG", "ART", "VMC"] {
        fs::create_dir_all(library.join(folder)).unwrap();
    }

    // Games OPL lists: an old-style CD, a dual-layer DVD, an uppercase
    // extension (warning), and a CD whose name disagrees with SYSTEM.CNF.
    write(
        &library.join("CD/SLUS_200.62.Retail Game.iso"),
        &Variant::Retail.build().to_vec(),
    );
    write(
        &library.join("DVD/SLUS_212.18.Dual Layer Quest.iso"),
        &Variant::Dvd9.build().to_vec(),
    );
    write(
        &library.join("CD/SLPM_123.45.Uppercase Ext.ISO"),
        &Variant::LongId.build().to_vec(),
    );
    write(
        &library.join("CD/SLES_999.99.Wrong Name.iso"),
        &Variant::Retail.build().to_vec(),
    );
    // Things OPL skips: no BOOT2, a PS1 disc, a stray text file.
    write(
        &library.join("DVD/No Boot2.iso"),
        &Variant::NoBoot2.build().to_vec(),
    );
    write(
        &library.join("CD/Ps1 Disc.iso"),
        &Variant::Ps1.build().to_vec(),
    );
    write(&library.join("CD/readme.txt"), b"not a game\n");
    // A CFG with compatibility modes and a VMC slot for the retail game; the
    // card file is a stub, which the scan flags (the real thing is VMC-1).
    write(
        &library.join("CFG/SLUS_200.62.cfg"),
        b"$Compatibility=5\r\n$VMC_0=SLUS_200.62_0\r\nTitle=Retail Game\r\n",
    );
    write(&library.join("VMC/SLUS_200.62_0.bin"), &vec![0u8; 64 << 10]);
    for n in 0..extra {
        let id = format!("SLUS_{:03}.{:02}", 300 + n / 100, n % 100);
        write(
            &library.join(format!("CD/{id}.Filler Game {n:04}.iso")),
            &Variant::Retail.build().to_vec(),
        );
    }

    // Import sources: a plain ISO, a BIN/CUE pair and a folder with one more.
    write(
        &sources.join("Lf Game.iso"),
        &Variant::LfOnly.build().to_vec(),
    );
    let iso = Variant::Dvd9.build().to_vec();
    write(
        &sources.join("Disc Image.bin"),
        &to_bin(&iso, RawMode::Mode2_2352, 0),
    );
    write(
        &sources.join("Disc Image.cue"),
        cue_sheet("Disc Image.bin", RawMode::Mode2_2352, 0).as_bytes(),
    );
    write(
        &sources.join("folder/SLUS_200.62.Retail Copy.iso"),
        &Variant::Retail.build().to_vec(),
    );
    write(&sources.join("notes.txt"), b"not an image\n");

    // A ul game, written the way YAOPLM writes it.
    let staged = dir.join("staging");
    let _ = fs::remove_dir_all(&staged);
    write(
        &staged.join("Split Game.iso"),
        &Variant::LfOnly.build().to_vec(),
    );
    import(&library, &[staged.join("Split Game.iso")], Layout::Ul);
    let _ = fs::remove_dir_all(&staged);
    // OPL's cache is deleted on every change; a fresh one here means a
    // stale-cache warning is never produced by accident.
    let _ = fs::remove_file(library.join("CD/games.bin"));

    println!("library: {}", library.display());
    println!("sources: {}", sources.display());
    ExitCode::SUCCESS
}
