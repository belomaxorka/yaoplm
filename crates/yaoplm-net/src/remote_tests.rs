//! The remote sources end to end against a local server: OPL Manager's
//! backup read by range requests from a small zip of the same layout, and
//! ScreenScraper answering from canned XML. No test here leaves the
//! machine.

use std::collections::HashMap;

use image::{DynamicImage, ImageBuffer, Rgb};
use yaoplm_formats::art::{ArtKind, ArtSize, png_header};

use crate::remote_zip::tests::build_zip;
use crate::screenscraper::{Account, Api, DevCredentials};
use crate::sources::{SourceConfig, SourceId};
use crate::test_server::{Reply, serve, serve_routes};
use crate::{ArtService, AttemptResult, CoverRequest, Endpoints, oplm};

fn png(width: u32, height: u32, seed: u8) -> Vec<u8> {
    let img = ImageBuffer::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, seed]));
    let mut out = Vec::new();
    DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

const fn size(width: u32, height: u32) -> ArtSize {
    ArtSize { width, height }
}

fn no_screenscraper() -> Api {
    Api {
        base: String::new(),
        dev: None,
    }
}

fn service_with(data: &std::path::Path, oplm: oplm::Archive, screenscraper: Api) -> ArtService {
    ArtService::open_with(
        data,
        Endpoints {
            oplm,
            screenscraper,
        },
    )
    .unwrap()
}

/// A backup with DOA2 in three markets: the US game has its icon, spine,
/// screenshots and background; the Japanese ID carries the US cover (the
/// wrong release, as in research/14 §3.3); the PAL game has its own.
fn backup() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let us_cover = png(140, 200, 1);
    let icon = png(64, 64, 2);
    let members = vec![
        ("PS2_LIST.csv", b"ID;Title".to_vec()),
        ("PS2/SLUS_200.71/SLUS_200.71_COV.png", us_cover.clone()),
        ("PS2/SLUS_200.71/SLUS_200.71_ICO.png", icon.clone()),
        ("PS2/SLUS_200.71/SLUS_200.71_LAB.png", png(18, 240, 4)),
        ("PS2/SLUS_200.71/SLUS_200.71_SCR_00.png", png(250, 188, 5)),
        ("PS2/SLUS_200.71/SLUS_200.71_SCR_01.png", png(250, 188, 6)),
        ("PS2/SLUS_200.71/SLUS_200.71_SCR_02.png", png(250, 188, 7)),
        ("PS2/SLUS_200.71/SLUS_200.71_BG_00.png", png(640, 480, 8)),
        ("PS2/SLPS_250.02/SLPS_250.02_COV.png", us_cover.clone()),
        ("PS2/SLPS_250.02/SLPS_250.02_SCR_02.png", png(250, 188, 9)),
        ("PS2/SCES_500.03/SCES_500.03_COV.png", png(140, 200, 3)),
    ];
    (build_zip(&members, true), us_cover, icon)
}

fn request<'a>(
    serial: &'a str,
    kind: ArtKind,
    size: ArtSize,
    sources: &'a [SourceConfig],
) -> CoverRequest<'a> {
    CoverRequest {
        serial,
        redump_names: &[],
        kind,
        size,
        sources,
        revalidate: false,
        others: &[],
    }
}

#[tokio::test]
async fn opl_manager_backup_by_range_requests() {
    let (zip, us_cover, icon) = backup();
    let mut routes = HashMap::new();
    // The item's own server answers its first request with a 500.
    routes.insert(
        "/items/TEST/TEST.zip".to_owned(),
        Reply {
            failures: 1,
            ..Reply::ok("application/zip", zip)
        },
    );
    let (base, seen, routes) = serve_routes(routes).await;
    // The download URL is gone; the metadata names the server that works
    // (and a host that is not one).
    let host = base.trim_start_matches("http://");
    routes.lock().unwrap().insert(
        "/metadata/TEST".to_owned(),
        Reply::ok(
            "application/json",
            format!(r#"{{"server":"{host}","d1":"bad host!","dir":"/items/TEST"}}"#).into_bytes(),
        ),
    );
    let archive = oplm::Archive {
        item: "TEST".into(),
        urls: vec![format!("{base}/download/TEST/TEST.zip")],
        metadata: Some(format!("{base}/metadata/TEST")),
    };
    let data = tempfile::tempdir().unwrap();
    let service = service_with(data.path(), archive.clone(), no_screenscraper());
    let sources = [SourceConfig::remote(SourceId::Oplm)];

    // An icon: the index first (tail and directory), then one range.
    let outcome = service
        .fetch_cover(&request("SLUS-20071", ArtKind::Ico, size(64, 64), &sources))
        .await;
    let cover = outcome
        .cover
        .unwrap_or_else(|| panic!("{:?}", outcome.attempts));
    assert_eq!(cover.source, "oplm");
    assert!(
        cover
            .origin
            .ends_with("/TEST.zip/PS2/SLUS_200.71/SLUS_200.71_ICO.png"),
        "{}",
        cover.origin
    );
    // A PNG at the slot's size is written as it is.
    assert_eq!(std::fs::read(&cover.png).unwrap(), icon);
    let count = |path: &str| seen.lock().unwrap().count(path);
    assert_eq!(count("/download/TEST/TEST.zip"), 1);
    assert_eq!(count("/metadata/TEST"), 1);
    // A 500, then the tail, the directory and the picture.
    assert_eq!(count("/items/TEST/TEST.zip"), 4);
    let ranges: Vec<String> = seen
        .lock()
        .unwrap()
        .requests
        .iter()
        .filter_map(|(_, h)| h.get("range").cloned())
        .collect();
    assert!(ranges[0].starts_with("bytes=-"), "{ranges:?}");

    // The next picture is one request, from the server that answered;
    // the cache answers the second time.
    let lab = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Lab,
            size(18, 240),
            &sources,
        ))
        .await;
    assert!(lab.cover.is_some(), "{:?}", lab.attempts);
    assert_eq!(count("/items/TEST/TEST.zip"), 5);
    assert_eq!(count("/download/TEST/TEST.zip"), 1);
    let again = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Lab,
            size(18, 240),
            &sources,
        ))
        .await;
    assert_eq!(again.attempts[0].result, AttemptResult::Cached);
    assert_eq!(count("/items/TEST/TEST.zip"), 5);

    // A type the backup lacks costs no request at all.
    let none = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Cov2,
            size(242, 344),
            &sources,
        ))
        .await;
    assert!(none.cover.is_none());
    assert_eq!(none.attempts[0].result, AttemptResult::Missing);
    assert_eq!(count("/items/TEST/TEST.zip"), 5);

    // The Japanese ID holds the US cover: left out, and never asked for.
    let jp = service
        .fetch_cover(&request(
            "SLPS-25002",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(jp.cover.is_none());
    assert_eq!(jp.attempts[0].result, AttemptResult::OtherMarket);
    assert_eq!(count("/items/TEST/TEST.zip"), 5);
    // The US game has the same bytes: one of the two is the wrong
    // release, so neither is taken silently.
    let us = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert_eq!(us.attempts[0].result, AttemptResult::OtherMarket);
    // By hand it is offered, last.
    let (variants, _) = service
        .find_variants(&request(
            "SLPS-25002",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert_eq!(variants.len(), 1);
    assert_eq!(std::fs::read(&variants[0].png).unwrap(), us_cover);
    // A background from a screenshot OPL shows nowhere else.
    let bg = service
        .fetch_cover(&request(
            "SLPS-25002",
            ArtKind::Bg,
            size(640, 480),
            &sources,
        ))
        .await
        .cover
        .unwrap();
    assert!(
        bg.origin.ends_with("SLPS_250.02_SCR_02.png"),
        "{}",
        bg.origin
    );
    let header = png_header(&std::fs::read(&bg.png).unwrap()).unwrap();
    assert_eq!((header.width, header.height), (640, 480));
    // Screenshots to pick from: SCR_00 first, then the numbered ones.
    let (shots, _) = service
        .find_variants(&request(
            "SLUS-20071",
            ArtKind::Scr,
            size(250, 188),
            &sources,
        ))
        .await;
    let origins: Vec<&str> = shots.iter().map(|c| c.origin.as_str()).collect();
    assert_eq!(origins.len(), 2, "{origins:?}");
    assert!(origins[0].ends_with("SCR_00.png") && origins[1].ends_with("SCR_02.png"));

    // A new process reads the index from disk: only the picture is asked.
    drop(service);
    let before = count("/items/TEST/TEST.zip") + count("/download/TEST/TEST.zip");
    let service = service_with(data.path(), archive, no_screenscraper());
    let pal = service
        .fetch_cover(&request(
            "SCES-50003",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(pal.cover.is_some(), "{:?}", pal.attempts);
    assert_eq!(
        count("/items/TEST/TEST.zip") + count("/download/TEST/TEST.zip"),
        before + 1
    );
}

#[tokio::test]
async fn a_changed_backup_drops_its_index() {
    let (zip, _, _) = backup();
    // The same directory over other bytes: SCR_01's CRC no longer holds.
    let name = b"PS2/SLUS_200.71/SLUS_200.71_SCR_01.png";
    let at = zip.windows(name.len()).position(|w| w == name).unwrap();
    let mut changed = zip.clone();
    changed[at + name.len() + 60] ^= 0xFF;
    let mut routes = HashMap::new();
    routes.insert("/z.zip".to_owned(), Reply::ok("application/zip", changed));
    let (base, seen) = serve(routes).await;
    let data = tempfile::tempdir().unwrap();
    let archive = oplm::Archive {
        item: "Z".into(),
        urls: vec![format!("{base}/z.zip")],
        metadata: None,
    };
    let service = service_with(data.path(), archive, no_screenscraper());
    let sources = [SourceConfig::remote(SourceId::Oplm)];
    let index_dir = data
        .path()
        .join("art-cache")
        .join(oplm::INDEX_DIR)
        .join("Z");
    let pal = service
        .fetch_cover(&request(
            "SCES-50003",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(pal.cover.is_some(), "{:?}", pal.attempts);
    assert!(index_dir.join("directory.bin").is_file());
    let broken = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Scr2,
            size(250, 188),
            &sources,
        ))
        .await;
    assert!(broken.cover.is_none());
    match &broken.attempts[0].result {
        AttemptResult::Error(err) => assert!(err.contains("CRC"), "{err}"),
        other => panic!("{other:?}"),
    }
    assert!(!index_dir.join("directory.bin").exists());
    // The next request reads the index again: tail, directory, picture.
    let before = seen.lock().unwrap().count("/z.zip");
    let lab = service
        .fetch_cover(&request(
            "SLUS-20071",
            ArtKind::Lab,
            size(18, 240),
            &sources,
        ))
        .await;
    assert!(lab.cover.is_some(), "{:?}", lab.attempts);
    assert_eq!(seen.lock().unwrap().count("/z.zip"), before + 3);
}

#[tokio::test]
async fn an_unreachable_backup_rests_for_a_while() {
    let (base, seen) = serve(HashMap::new()).await;
    let data = tempfile::tempdir().unwrap();
    let archive = oplm::Archive {
        item: "GONE".into(),
        urls: vec![format!("{base}/gone.zip")],
        metadata: None,
    };
    let service = service_with(data.path(), archive, no_screenscraper());
    let sources = [SourceConfig::remote(SourceId::Oplm)];
    let first = service
        .fetch_cover(&request("SLUS-20071", ArtKind::Ico, size(64, 64), &sources))
        .await;
    assert!(
        matches!(&first.attempts[0].result, AttemptResult::Error(e) if e.contains("404")),
        "{first:?}"
    );
    let asked = seen.lock().unwrap().requests.len();
    let second = service
        .fetch_cover(&request("SLES-50330", ArtKind::Ico, size(64, 64), &sources))
        .await;
    assert!(
        matches!(&second.attempts[0].result, AttemptResult::Error(e) if e.contains("not asked again")),
        "{second:?}"
    );
    assert_eq!(seen.lock().unwrap().requests.len(), asked);
}

fn screenscraper_xml(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <ssuser><id>guest</id><requeststoday>3</requeststoday><maxrequestsperday>20000</maxrequestsperday></ssuser>
  <jeu id="3940">
    <medias>
      <media type="box-2D" region="eu" format="png">{base}/media/box-eu.png?devid=dev&amp;devpassword=s3cret&amp;media=box-2D(eu)</media>
      <media type="box-2D" region="jp" format="png">{base}/media/box-jp.png?devid=dev&amp;devpassword=s3cret&amp;media=box-2D(jp)</media>
      <media type="support-2D" region="jp" format="png">{base}/media/disc-jp.png?devid=dev&amp;devpassword=s3cret</media>
      <media type="box-2D-side" region="us" format="png">{base}/media/side-us.png?devid=dev&amp;devpassword=s3cret</media>
    </medias>
  </jeu>
</Data>"#
    )
}

#[tokio::test]
async fn screenscraper_by_serial_and_market() {
    let mut routes = HashMap::new();
    routes.insert(
        "/media/box-jp.png".to_owned(),
        Reply::ok("image/png", png(300, 420, 1)),
    );
    routes.insert(
        "/media/box-eu.png".to_owned(),
        Reply::ok("image/png", png(300, 420, 2)),
    );
    routes.insert(
        "/media/disc-jp.png".to_owned(),
        Reply::ok("text/plain", b"NOMEDIA".to_vec()),
    );
    let (base, seen, routes) = serve_routes(routes).await;
    routes.lock().unwrap().insert(
        "/api2/jeuInfos.php".to_owned(),
        Reply::ok("application/xml", screenscraper_xml(&base).into_bytes()),
    );
    let api = Api {
        base: format!("{base}/api2"),
        dev: Some(DevCredentials {
            devid: "dev".into(),
            devpassword: "s3cret".into(),
            softname: "YAOPLM test".into(),
        }),
    };
    let account = Account {
        user: "me".into(),
        password: "pw".into(),
    };
    // Another serial runs into the daily quota.
    let quota = api.info_url(Some(&account), "SLUS-29999").unwrap();
    routes.lock().unwrap().insert(
        quota.trim_start_matches(&base).to_owned(),
        Reply {
            status: 430,
            ..Reply::default()
        },
    );
    let data = tempfile::tempdir().unwrap();
    let service = service_with(data.path(), oplm::Archive::default(), api);
    assert!(service.screenscraper_ready());
    let mut config = SourceConfig::remote(SourceId::ScreenScraper);
    config.account = Some(account);
    let sources = [config];

    // The Japanese box for the Japanese disc.
    let jp = service
        .fetch_cover(&request(
            "SLPS-25002",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    let cover = jp.cover.unwrap_or_else(|| panic!("{:?}", jp.attempts));
    assert_eq!(cover.source, "screenscraper");
    assert_eq!(cover.license_tag, "CC BY-NC-SA 4.0");
    assert!(cover.origin.contains("box-jp.png"));
    // No credential shows in the log or the cache.
    let shown = format!("{:?}", jp.attempts);
    assert!(
        !shown.contains("s3cret") && !shown.contains("pw"),
        "{shown}"
    );
    let info = service
        .cache()
        .get("screenscraper", "jeu/SLPS-25002")
        .unwrap()
        .unwrap();
    assert!(!info.url.contains("s3cret") && info.url.contains("devpassword=***"));
    assert!(!cover.origin.contains("s3cret"));
    // The account went with the request.
    let asked = seen.lock().unwrap().requests[0].0.clone();
    assert!(
        asked.contains("ssid=me&sspassword=pw") && asked.contains("serialnum=SLPS-25002"),
        "{asked}"
    );

    // The disc: NOMEDIA is "not found". The media list came from the
    // cache: one jeuInfos request for the game.
    let disc = service
        .fetch_cover(&request("SLPS-25002", ArtKind::Ico, size(64, 64), &sources))
        .await;
    assert!(disc.cover.is_none());
    assert_eq!(disc.attempts.last().unwrap().result, AttemptResult::Missing);
    assert_eq!(seen.lock().unwrap().count("/api2/jeuInfos.php"), 1);
    // Only a US spine: not for the Japanese game.
    let spine = service
        .fetch_cover(&request(
            "SLPS-25002",
            ArtKind::Lab,
            size(18, 240),
            &sources,
        ))
        .await;
    assert_eq!(spine.attempts[0].result, AttemptResult::OtherMarket);

    // The quota is used up: the source rests, nothing more is asked.
    let spent = service
        .fetch_cover(&request(
            "SLUS-29999",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(matches!(&spent.attempts[0].result, AttemptResult::Error(e) if e.contains("quota")));
    let before = seen.lock().unwrap().requests.len();
    let resting = service
        .fetch_cover(&request(
            "SCES-50003",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(matches!(&resting.attempts[0].result, AttemptResult::Error(e) if e.contains("quota")));
    assert_eq!(seen.lock().unwrap().requests.len(), before);

    // Without a developer key the source is simply not there.
    let empty = tempfile::tempdir().unwrap();
    let bare = service_with(empty.path(), oplm::Archive::default(), no_screenscraper());
    assert!(!bare.screenscraper_ready());
    let nothing = bare
        .fetch_cover(&request(
            "SLPS-25002",
            ArtKind::Cov,
            size(140, 200),
            &sources,
        ))
        .await;
    assert!(nothing.cover.is_none() && nothing.attempts.is_empty());
}
