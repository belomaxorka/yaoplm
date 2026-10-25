//! Builds the fixture library the Playwright tests run against
//! (TESTING.md, "Проверка интерфейса агентом"):
//!
//! ```text
//! cargo run -p yaoplm-api --example make_library -- <folder> [--games N]
//! ```
//!
//! `<folder>/library` is a device root with games in every format and a
//! few things to warn about; `<folder>/sources` holds images to import;
//! `<folder>/mirror` holds local mirrors of three picture sources with a
//! picture each (ART-1, ADR-0035), so the art screens work without the
//! network, and a
//! Redump DAT that verifies some of the games and not others (GAME-5),
//! `sets/` with two picture sets to import (ART-5), `loaders/`, the
//! catalogue builds' GitHub releases with test ELFs (LDR-2, LDR-3), and
//! `loaders-next/`, the same with upstream's beta rebuilt as build 2280
//! (LDR-4: "Check for updates" against it finds an update), and `own/`
//! with OPL 1.1.0's ELF to add as the user's own file (LDR-7: the catalogue
//! pins the real 1.1.0 archive, which no fixture has);
//! `<folder>/stick` is an empty device to install a loader on, and
//! `<folder>/extras/` holds games to copy onto it: a ZSO and an ISO with a
//! JPG cover (LDR-7). Pass
//! `--games N` to add N more games for virtualization checks. Nothing here
//! is a real game or a real cover: every image is generated.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use image::{DynamicImage, ImageBuffer, Rgb};
use yaoplm_api::jobs::{JobResult, JobStatus};
use yaoplm_api::{Api, Layout, Request, Response};
use yaoplm_test_support::Variant;
use yaoplm_test_support::bincue::{RawMode, cue_sheet, to_bin};
use yaoplm_test_support::dat::DatBuilder;
use yaoplm_test_support::loaders::ReleaseMirror;
use yaoplm_test_support::packed;

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

/// A flat two-tone "cover" with a diagonal, so a resized copy is still
/// recognisable in a screenshot.
fn cover(width: u32, height: u32, hue: u8, format: image::ImageFormat) -> Vec<u8> {
    let img = ImageBuffer::from_fn(width, height, |x, y| {
        let band = (x + y) / 24 % 2 == 0;
        if band {
            Rgb([hue, 80, 200u8.wrapping_sub(hue)])
        } else {
            Rgb([240, 240, 230])
        }
    });
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut Cursor::new(&mut out), format)
        .unwrap();
    out
}

/// Imports `sources` into `root` through the API, like the UI does.
fn import(root: &Path, sources: &[PathBuf], layout: Layout) {
    let api = Api::new();
    let Response::Plan(plan) = api
        .dispatch(Request::PlanImport {
            verify: None,
            database_titles: None,
            covers: None,
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
            compress: None,
            mode: None,
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
    let mirror = dir.join("mirror");
    let stick = dir.join("stick");
    let _ = fs::remove_dir_all(&library);
    let _ = fs::remove_dir_all(&sources);
    let _ = fs::remove_dir_all(&mirror);
    let _ = fs::remove_dir_all(&stick);
    fs::create_dir_all(&stick).unwrap();
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
    // One game already has a cover, so "only missing" has something to skip.
    write(
        &library.join("ART/SLUS_212.18_COV.png"),
        &cover(140, 200, 40, image::ImageFormat::Png),
    );

    // Cover mirrors: xlenore has the retail game by serial, libretro has
    // the split game (SCES-50003, Dead or Alive 2) by its Redump name.
    write(
        &mirror.join("xlenore/covers/default/SLUS-20062.jpg"),
        &cover(300, 420, 200, image::ImageFormat::Jpeg),
    );
    write(
        &mirror.join("libretro/Named_Boxarts/Dead or Alive 2 (Europe) (En,Fr,De,Es,It).png"),
        &cover(256, 362, 110, image::ImageFormat::Png),
    );
    // OPL Manager's backup, in its own layout, has the split game's disc
    // icon: a type no other source has.
    write(
        &mirror.join("oplm/PS2/SCES_500.03/SCES_500.03_ICO.png"),
        &cover(64, 64, 75, image::ImageFormat::Png),
    );
    // A CD to add, dumped raw as Redump does: xlenore has its cover and the
    // DAT its data track, so adding it writes the cover and verifies it
    // (ART-8, GAME-5).
    let new_game = to_bin(
        &yaoplm_test_support::fixtures::boot_iso(
            yaoplm_test_support::fixtures::placeholder_elf(),
            "SLUS_299.99",
            0,
        )
        .build()
        .to_vec(),
        RawMode::Mode2_2352,
        0,
    );
    write(
        &mirror.join("xlenore/covers/default/SLUS-29999.jpg"),
        &cover(300, 420, 160, image::ImageFormat::Jpeg),
    );
    // A Redump DAT to import (GAME-5): the retail game and the DVD-9 game
    // match, the split game's ID is listed with other bytes (a bad dump),
    // the uppercase one is not listed at all.
    let mut lf_damaged = Variant::LfOnly.build().to_vec();
    let last = lf_damaged.len() - 1;
    lf_damaged[last] ^= 0xFF;
    write(
        &mirror.join("redump/Sony - PlayStation 2 - Datfile (4) (2026-10-23 01-56-22).dat"),
        DatBuilder::new()
            .iso(
                "Retail Game (USA)",
                Some("SLUS-20062, SLUS-20062GH"),
                &Variant::Retail.build().to_vec(),
            )
            .iso(
                "Dual Layer Quest (USA)",
                Some("SLUS 21218"),
                &Variant::Dvd9.build().to_vec(),
            )
            .iso(
                "Dead or Alive 2 (Europe) (En,Fr,De,Es,It)",
                Some("SCES-50003"),
                &lf_damaged,
            )
            .bin_cue("New Game (USA)", Some("SLUS-29999"), &new_game)
            .build()
            .as_bytes(),
    );
    // A loose image to pick as a manual replacement (ART-3).
    write(
        &mirror.join("custom-cover.jpg"),
        &cover(350, 500, 20, image::ImageFormat::Jpeg),
    );
    // Picture sets (ART-5): an OPL Manager art backup (`PS2/<ID>/` with
    // numbered screenshots) and an `art.tar`. One picture is for a game
    // that already has a cover and one for a game not on the device.
    let jpg = |w, h, hue| cover(w, h, hue, image::ImageFormat::Jpeg);
    let (icon, screen, front, other) = (
        jpg(64, 64, 90),
        jpg(320, 240, 120),
        jpg(140, 200, 60),
        jpg(140, 200, 150),
    );
    write(
        &mirror.join("sets/OPLM art backup.zip"),
        &packed::zip(
            &[
                ("PS2/SLUS_200.62/SLUS_200.62_ICO.jpg", &icon),
                ("PS2/SLUS_200.62/SLUS_200.62_SCR_00.jpg", &screen),
                ("PS2/SLUS_212.18/SLUS_212.18_COV.jpg", &front),
                ("PS2/SLES_888.88/SLES_888.88_COV.jpg", &other),
            ],
            false,
        )
        .unwrap(),
    );
    write(
        &mirror.join("sets/art.tar"),
        &packed::tar(&[(
            "SLUS_212.18_COV2.png",
            &cover(140, 200, 30, image::ImageFormat::Png),
        )]),
    );
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
    write(&sources.join("New Game.bin"), &new_game);
    write(
        &sources.join("New Game.cue"),
        cue_sheet("New Game.bin", RawMode::Mode2_2352, 0).as_bytes(),
    );
    write(&sources.join("notes.txt"), b"not an image\n");

    // Loader releases: the installer reads them instead of GitHub.
    ReleaseMirror::new(&mirror.join("loaders"))
        .official()
        .expect("loader releases");
    let next = ReleaseMirror::new(&mirror.join("loaders-next"));
    next.official().expect("loader releases");
    next.beta("v1.2.0-Beta-2280-0123abc", "2026-10-20")
        .expect("a newer beta");
    write(
        &mirror.join("own/OPNPS2LD-v1.1.0.ELF"),
        &yaoplm_test_support::loaders::opl_elf("v1.1.0"),
    );
    // Games for a stick whose loader is replaced: a ZSO OPL 1.1.0 cannot
    // show, an ISO with a JPG cover the betas cannot load.
    let extras = dir.join("extras");
    let _ = fs::remove_dir_all(&extras);
    let retail_iso = Variant::Retail.build().to_vec();
    let mut packed = Cursor::new(Vec::new());
    yaoplm_formats::zso::compress(
        &mut Cursor::new(&retail_iso),
        retail_iso.len() as u64,
        &mut packed,
        &mut |_| {},
    )
    .expect("a ZSO");
    write(
        &extras.join("DVD/SLUS_200.62.Retail Game.zso"),
        &packed.into_inner(),
    );
    write(
        &extras.join("CD/SLUS_212.18.Lf Game.iso"),
        &Variant::LfOnly.build().to_vec(),
    );
    write(
        &extras.join("ART/SLUS_212.18_COV.jpg"),
        &cover(140, 200, 90, image::ImageFormat::Jpeg),
    );

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
    println!("mirror: {}", mirror.display());
    println!("stick: {}", stick.display());
    ExitCode::SUCCESS
}
