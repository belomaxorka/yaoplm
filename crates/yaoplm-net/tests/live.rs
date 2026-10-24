//! The picture sources on the real network (ADR-0035, research/14): run by
//! hand before a release or when a source seems broken, never in CI.
//!
//! ```text
//! YAOPLM_LIVE=1 cargo test -p yaoplm-net --test live -- --ignored --nocapture
//! ```
//!
//! Dead or Alive 2 in three regions: OPL Manager's backup on archive.org
//! (its 8.6 MB index once, then one range request a picture), xlenore and
//! libretro. ScreenScraper too when `YAOPLM_SCREENSCRAPER_DEVID` and
//! `_DEVPASSWORD` are set. Everything goes to a temporary cache, a second
//! apart per source.

use yaoplm_formats::art::{ArtKind, ArtSize, ArtSubject, png_header, recommended_size};
use yaoplm_formats::profile::LoaderProfile;
use yaoplm_net::sources::{SourceConfig, SourceId};
use yaoplm_net::{ArtService, AttemptResult, CoverRequest};

fn size(kind: ArtKind) -> ArtSize {
    recommended_size(
        LoaderProfile::Opl120Beta,
        ArtSubject::Game { startup: "" },
        kind,
    )
}

fn live() -> bool {
    std::env::var_os("YAOPLM_LIVE").is_some()
}

#[tokio::test]
#[ignore = "network: YAOPLM_LIVE=1"]
async fn opl_manager_backup_on_archive_org() {
    if !live() {
        eprintln!("YAOPLM_LIVE is not set");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let service = ArtService::open(data.path()).unwrap();
    let sources = [SourceConfig::remote(SourceId::Oplm)];
    // Every type of the US game, then packaging of the PAL and Japanese ones.
    let wanted = [
        ("SLUS-20071", ArtKind::Ico),
        ("SLUS-20071", ArtKind::Cov),
        ("SLUS-20071", ArtKind::Cov2),
        ("SLUS-20071", ArtKind::Lab),
        ("SLUS-20071", ArtKind::Bg),
        ("SLUS-20071", ArtKind::Scr),
        ("SLUS-20071", ArtKind::Scr2),
        ("SLUS-20071", ArtKind::Lgo),
        ("SCES-50003", ArtKind::Lab),
        ("SLPS-25002", ArtKind::Ico),
    ];
    for (serial, kind) in wanted {
        let started = std::time::Instant::now();
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial,
                redump_names: &[],
                kind,
                size: size(kind),
                sources: &sources,
                revalidate: false,
                others: &[],
            })
            .await;
        let cover = outcome
            .cover
            .unwrap_or_else(|| panic!("{serial} {kind}: {:?}", outcome.attempts));
        let png = std::fs::read(&cover.png).unwrap();
        let header = png_header(&png).unwrap();
        println!(
            "{serial} {kind}: {}×{} from {} in {:.1} s",
            header.width,
            header.height,
            cover.origin,
            started.elapsed().as_secs_f32()
        );
        assert_eq!((header.width, header.height), {
            let s = size(kind);
            (s.width, s.height)
        });
    }
    let (rows, bytes) = service.cache().stats().unwrap();
    println!("cache: {rows} pictures, {bytes} bytes (with the index)");
}

#[tokio::test]
#[ignore = "network: YAOPLM_LIVE=1"]
async fn xlenore_libretro_and_screenscraper() {
    if !live() {
        eprintln!("YAOPLM_LIVE is not set");
        return;
    }
    let data = tempfile::tempdir().unwrap();
    let service = ArtService::open(data.path()).unwrap();
    let names = ["Dead or Alive 2 (Japan)".to_owned()];
    for (source, serial, kind, names) in [
        (SourceId::Xlenore, "SLPS-25002", ArtKind::Cov, &names[..]),
        (SourceId::Libretro, "SLPS-25002", ArtKind::Scr, &names[..]),
        (SourceId::Libretro, "SLPS-25002", ArtKind::Lgo, &names[..]),
    ] {
        let sources = [SourceConfig::remote(source)];
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial,
                redump_names: names,
                kind,
                size: size(kind),
                sources: &sources,
                revalidate: false,
                others: &[],
            })
            .await;
        let cover = outcome
            .cover
            .unwrap_or_else(|| panic!("{source:?} {kind}: {:?}", outcome.attempts));
        println!("{} {serial} {kind}: {}", source.as_str(), cover.origin);
    }
    if !service.screenscraper_ready() {
        println!("ScreenScraper: no developer key, skipped");
        return;
    }
    let sources = [SourceConfig::remote(SourceId::ScreenScraper)];
    for kind in ArtKind::ALL {
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial: "SLPS-25002",
                redump_names: &[],
                kind,
                size: size(kind),
                sources: &sources,
                revalidate: false,
                others: &[],
            })
            .await;
        let result = match &outcome.cover {
            Some(cover) => format!("found: {}", cover.origin),
            None => format!(
                "none: {:?}",
                outcome
                    .attempts
                    .last()
                    .map(|a| &a.result)
                    .unwrap_or(&AttemptResult::Missing)
            ),
        };
        println!("screenscraper SLPS-25002 {kind}: {result}");
    }
}
