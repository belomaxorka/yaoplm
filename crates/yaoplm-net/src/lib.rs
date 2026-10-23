//! Cover-art sources, the download cache, request limits and PNG
//! transcoding (ART-1, ART-2, ADR-0007), and the Redump DAT for dump
//! verification (META-2).
//!
//! Asynchronous on purpose: the core never uses this crate (ADR-0003);
//! `yaoplm-api` drives [`ArtService`] from a job thread through a tokio
//! runtime. Network access happens only inside [`ArtService::fetch_cover`],
//! which a user action starts, and every answer is cached
//! (ARCHITECTURE.md §6-§7).

pub mod cache;
pub mod http;
pub mod redump;
pub mod sources;
pub mod transcode;

use std::path::{Path, PathBuf};

use yaoplm_formats::art::ArtSize;

use cache::{Cache, Download, Record, Status};
use http::{Fetched, Http};
use sources::{Candidate, Location, SourceConfig, SourceId};

/// At most this many Redump names are tried per game: each is a request.
pub const MAX_LIBRETRO_CANDIDATES: usize = 3;
/// The pseudo-source of covers the user supplied (file, drop, paste).
pub const LOCAL_SOURCE: &str = "local";

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Http(String),
    #[error("{url}: HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{0}: the response is too large to be a cover")]
    TooLarge(String),
    #[error("image: {0}")]
    Image(String),
    #[error("cache: {0}")]
    Cache(String),
    #[error("Redump DAT: {0}")]
    Dat(String),
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl NetError {
    fn io(path: &Path) -> impl FnOnce(std::io::Error) -> Self + '_ {
        move |source| Self::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl From<rusqlite::Error> for NetError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Cache(err.to_string())
    }
}

/// One game's cover to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverRequest<'a> {
    /// `SLUS-20062`.
    pub serial: &'a str,
    /// Redump names in the order worth trying (region first).
    pub redump_names: &'a [String],
    /// The profile's COV size.
    pub size: ArtSize,
    /// Sources in priority order; an empty list means nothing is asked.
    pub sources: &'a [SourceConfig],
    /// Ask the server even when the cache has an answer (conditional
    /// request); "overwrite" runs set this.
    pub revalidate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttemptResult {
    /// Served from the cache without a request.
    Cached,
    Downloaded,
    /// 304: the cached copy is current.
    NotModified,
    /// 404, a missing mirror file, or a cached "not found".
    Missing,
    Error(String),
}

/// What happened at one source, for the job log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub source: String,
    /// The URL or mirror path that was tried.
    pub target: String,
    pub result: AttemptResult,
}

/// A cover ready to write: a PNG file in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub png: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source: String,
    pub key: String,
    /// Where the original came from (URL or mirror path).
    pub origin: String,
    pub license_tag: String,
    pub attribution: String,
    pub from_cache: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverOutcome {
    pub cover: Option<Cover>,
    pub attempts: Vec<Attempt>,
}

/// Finds covers and keeps them in the cache.
#[derive(Debug)]
pub struct ArtService {
    http: Http,
    cache: Cache,
}

impl ArtService {
    pub fn open(data_dir: &Path) -> Result<Self, NetError> {
        Ok(Self {
            http: Http::new()?,
            cache: Cache::open(data_dir)?,
        })
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// The shared client, for other downloads (the Redump DAT).
    pub fn http(&self) -> &Http {
        &self.http
    }

    fn derived_label(size: ArtSize) -> String {
        format!("COV-{}x{}", size.width, size.height)
    }

    fn cover_from(
        &self,
        record: &Record,
        size: ArtSize,
        from_cache: bool,
        origin: &str,
    ) -> Result<Cover, NetError> {
        let png = self
            .cache
            .derived(record, &Self::derived_label(size), |bytes| {
                transcode::to_png(bytes, size).map(|t| t.png)
            })?;
        Ok(Cover {
            png,
            width: size.width,
            height: size.height,
            source: record.source.clone(),
            key: record.key.clone(),
            origin: origin.to_owned(),
            license_tag: record.license_tag.clone(),
            attribution: record.attribution.clone(),
            from_cache,
        })
    }

    /// Tries the sources in order and returns the first cover found, with
    /// every attempt made along the way.
    pub async fn fetch_cover(&self, request: &CoverRequest<'_>) -> CoverOutcome {
        let mut attempts = Vec::new();
        for config in request.sources {
            let candidates = match config.id {
                SourceId::Xlenore => sources::xlenore_candidates(request.serial),
                SourceId::Libretro => {
                    let names: Vec<String> = request
                        .redump_names
                        .iter()
                        .take(MAX_LIBRETRO_CANDIDATES)
                        .cloned()
                        .collect();
                    sources::libretro_candidates(&names)
                }
            };
            for candidate in &candidates {
                let found = match &config.location {
                    Location::Mirror(dir) => {
                        self.via_mirror(config.id, candidate, dir, request.size, &mut attempts)
                    }
                    Location::Remote => {
                        self.via_remote(
                            config.id,
                            candidate,
                            request.size,
                            request.revalidate,
                            &mut attempts,
                        )
                        .await
                    }
                };
                if let Some(cover) = found {
                    return CoverOutcome {
                        cover: Some(cover),
                        attempts,
                    };
                }
            }
        }
        CoverOutcome {
            cover: None,
            attempts,
        }
    }

    fn via_mirror(
        &self,
        source: SourceId,
        candidate: &Candidate,
        dir: &Path,
        size: ArtSize,
        attempts: &mut Vec<Attempt>,
    ) -> Option<Cover> {
        let path = candidate.mirror_path(dir);
        let target = path.display().to_string();
        let mut attempt = |result: AttemptResult| {
            attempts.push(Attempt {
                source: source.as_str().to_owned(),
                target: target.clone(),
                result,
            });
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                attempt(AttemptResult::Missing);
                return None;
            }
            Err(err) => {
                attempt(AttemptResult::Error(err.to_string()));
                return None;
            }
        };
        if !transcode::looks_like_image(&bytes) {
            attempt(AttemptResult::Error("not an image".into()));
            return None;
        }
        let info = source.info();
        let stored = self.cache.put(
            source.as_str(),
            &candidate.key,
            &Download {
                url: &target,
                bytes: &bytes,
                content_type: None,
                etag: None,
                last_modified: None,
                license_tag: info.license_tag,
                attribution: info.attribution,
            },
        );
        let record = match stored {
            Ok(record) => record,
            Err(err) => {
                attempt(AttemptResult::Error(err.to_string()));
                return None;
            }
        };
        match self.cover_from(&record, size, false, &target) {
            Ok(cover) => {
                attempt(AttemptResult::Downloaded);
                Some(cover)
            }
            Err(err) => {
                attempt(AttemptResult::Error(err.to_string()));
                None
            }
        }
    }

    async fn via_remote(
        &self,
        source: SourceId,
        candidate: &Candidate,
        size: ArtSize,
        revalidate: bool,
        attempts: &mut Vec<Attempt>,
    ) -> Option<Cover> {
        let id = source.as_str();
        let info = source.info();
        let record = match self.cache.get(id, &candidate.key) {
            Ok(record) => record,
            Err(err) => {
                attempts.push(Attempt {
                    source: id.to_owned(),
                    target: candidate.urls.first().cloned().unwrap_or_default(),
                    result: AttemptResult::Error(err.to_string()),
                });
                return None;
            }
        };
        let now = cache::now_secs();
        if let Some(record) = &record
            && !revalidate
        {
            match record.status {
                Status::Ok if self.cache.original_path(record).is_some() => {
                    let result = self.cover_from(record, size, true, &record.url);
                    attempts.push(Attempt {
                        source: id.to_owned(),
                        target: record.url.clone(),
                        result: match &result {
                            Ok(_) => AttemptResult::Cached,
                            Err(err) => AttemptResult::Error(err.to_string()),
                        },
                    });
                    return result.ok();
                }
                Status::Missing if !record.is_stale_missing(now) => {
                    attempts.push(Attempt {
                        source: id.to_owned(),
                        target: record.url.clone(),
                        result: AttemptResult::Missing,
                    });
                    return None;
                }
                _ => {}
            }
        }
        let validators = record
            .as_ref()
            .filter(|r| r.status == Status::Ok && self.cache.original_path(r).is_some())
            .map(Record::validators)
            .unwrap_or_default();

        let mut last_missing: Option<String> = None;
        for url in &candidate.urls {
            let fetched = self.http.get(id, url, &validators).await;
            let (result, cover) = match fetched {
                Ok(Fetched::NotModified) => match record.as_ref().map(|r| self.cache.touch(r)) {
                    Some(Ok(fresh)) => match self.cover_from(&fresh, size, true, url) {
                        Ok(cover) => (AttemptResult::NotModified, Some(cover)),
                        Err(err) => (AttemptResult::Error(err.to_string()), None),
                    },
                    Some(Err(err)) => (AttemptResult::Error(err.to_string()), None),
                    None => (
                        AttemptResult::Error("304 without a cached copy".into()),
                        None,
                    ),
                },
                Ok(Fetched::NotFound) => {
                    last_missing = Some(url.clone());
                    (AttemptResult::Missing, None)
                }
                Ok(Fetched::Ok(body)) => {
                    if !transcode::looks_like_image(&body.bytes) {
                        (
                            AttemptResult::Error("the response is not an image".into()),
                            None,
                        )
                    } else {
                        let stored = self.cache.put(
                            id,
                            &candidate.key,
                            &Download {
                                url,
                                bytes: &body.bytes,
                                content_type: body.content_type.as_deref(),
                                etag: body.etag.as_deref(),
                                last_modified: body.last_modified.as_deref(),
                                license_tag: info.license_tag,
                                attribution: info.attribution,
                            },
                        );
                        match stored.and_then(|r| self.cover_from(&r, size, false, url)) {
                            Ok(cover) => (AttemptResult::Downloaded, Some(cover)),
                            Err(err) => (AttemptResult::Error(err.to_string()), None),
                        }
                    }
                }
                Err(err) => (AttemptResult::Error(err.to_string()), None),
            };
            let stop = cover.is_some()
                || !matches!(result, AttemptResult::Missing | AttemptResult::Error(_));
            attempts.push(Attempt {
                source: id.to_owned(),
                target: url.clone(),
                result,
            });
            if let Some(cover) = cover {
                return Some(cover);
            }
            if stop {
                return None;
            }
        }
        if let Some(url) = last_missing {
            // Every URL said 404: remember it for a week.
            let _ = self.cache.put_missing(id, &candidate.key, &url);
        }
        None
    }

    /// Stores a cover the user supplied (ART-3) and derives the PNG for
    /// `size`. `label` is kept as the origin for the log.
    pub fn import_local(
        &self,
        bytes: &[u8],
        size: ArtSize,
        label: &str,
    ) -> Result<Cover, NetError> {
        if !transcode::looks_like_image(bytes) {
            return Err(NetError::Image("not an image".into()));
        }
        let key = cache::sha256_hex(bytes);
        let record = self.cache.put(
            LOCAL_SOURCE,
            &key,
            &Download {
                url: label,
                bytes,
                content_type: None,
                etag: None,
                last_modified: None,
                license_tag: "user",
                attribution: "supplied by the user",
            },
        )?;
        self.cover_from(&record, size, false, label)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use image::{DynamicImage, ImageBuffer, Rgb};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use yaoplm_formats::art::png_header;

    use super::*;

    fn jpeg(width: u32, height: u32) -> Vec<u8> {
        let img = ImageBuffer::from_fn(width, height, |x, y| Rgb([x as u8, y as u8, 7]));
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut out),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        out
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = ImageBuffer::from_fn(width, height, |x, y| Rgb([y as u8, x as u8, 9]));
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    const SIZE: ArtSize = ArtSize {
        width: 140,
        height: 200,
    };

    /// A canned answer of the test server.
    #[derive(Clone)]
    struct Reply {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: Vec<u8>,
    }

    struct Seen {
        requests: Vec<(String, HashMap<String, String>)>,
        times: Vec<Instant>,
    }

    /// The smallest HTTP/1.1 server that satisfies reqwest: one request
    /// per connection, routed by path.
    async fn serve(routes: HashMap<String, Reply>) -> (String, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Seen {
            requests: Vec::new(),
            times: Vec::new(),
        }));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    let mut lines = text.lines();
                    let path = lines
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or("/")
                        .to_owned();
                    let headers: HashMap<String, String> = lines
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
                        .collect();
                    {
                        let mut seen = log.lock().unwrap();
                        seen.requests.push((path.clone(), headers.clone()));
                        seen.times.push(Instant::now());
                    }
                    let reply = routes.get(&path).cloned().unwrap_or(Reply {
                        status: 404,
                        headers: Vec::new(),
                        body: b"nope".to_vec(),
                    });
                    // A conditional request for the current ETag gets 304.
                    let etag = reply
                        .headers
                        .iter()
                        .find(|(k, _)| *k == "ETag")
                        .map(|(_, v)| v.clone());
                    let (status, body) = match (etag, headers.get("if-none-match")) {
                        (Some(etag), Some(sent)) if *sent == etag && reply.status == 200 => {
                            (304, Vec::new())
                        }
                        _ => (reply.status, reply.body.clone()),
                    };
                    let mut out = format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                        body.len()
                    );
                    for (k, v) in &reply.headers {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str("\r\n");
                    let _ = socket.write_all(out.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, seen)
    }

    fn service() -> (tempfile::TempDir, ArtService) {
        let dir = tempfile::tempdir().unwrap();
        let service = ArtService::open(dir.path()).unwrap();
        (dir, service)
    }

    /// Candidate URLs are built for the public hosts; tests swap them for
    /// the local server through a direct `via_remote` call.
    async fn fetch_url(
        service: &ArtService,
        source: SourceId,
        key: &str,
        urls: Vec<String>,
        revalidate: bool,
    ) -> (Option<Cover>, Vec<Attempt>) {
        let candidate = Candidate {
            key: key.to_owned(),
            path: String::new(),
            urls,
        };
        let mut attempts = Vec::new();
        let cover = service
            .via_remote(source, &candidate, SIZE, revalidate, &mut attempts)
            .await;
        (cover, attempts)
    }

    #[tokio::test]
    async fn downloads_caches_and_revalidates() {
        let mut routes = HashMap::new();
        routes.insert(
            "/covers/default/SLUS-20062.jpg".to_owned(),
            Reply {
                status: 200,
                headers: vec![
                    ("Content-Type", "image/jpeg".into()),
                    ("ETag", "\"v1\"".into()),
                ],
                body: jpeg(300, 420),
            },
        );
        let (base, seen) = serve(routes).await;
        let (_dir, service) = service();
        let url = format!("{base}/covers/default/SLUS-20062.jpg");

        let (cover, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "SLUS-20062",
            vec![url.clone()],
            false,
        )
        .await;
        let cover = cover.unwrap();
        assert_eq!(attempts.last().unwrap().result, AttemptResult::Downloaded);
        assert!(!cover.from_cache);
        let bytes = std::fs::read(&cover.png).unwrap();
        let header = png_header(&bytes).unwrap();
        assert_eq!((header.width, header.height), (140, 200));
        assert_eq!(cover.source, "xlenore");
        assert_eq!(cover.license_tag, "unlicensed");
        {
            let seen = seen.lock().unwrap();
            assert_eq!(seen.requests.len(), 1);
            let ua = seen.requests[0].1.get("user-agent").unwrap();
            assert!(ua.starts_with("YAOPLM/"), "{ua}");
            assert!(
                ua.contains("+https://github.com/belomaxorka/yaoplm"),
                "{ua}"
            );
        }

        // Second time: no request at all.
        let (again, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "SLUS-20062",
            vec![url.clone()],
            false,
        )
        .await;
        assert!(again.unwrap().from_cache);
        assert_eq!(attempts.last().unwrap().result, AttemptResult::Cached);
        assert_eq!(seen.lock().unwrap().requests.len(), 1);

        // Revalidation sends If-None-Match and gets a 304.
        let (third, attempts) =
            fetch_url(&service, SourceId::Xlenore, "SLUS-20062", vec![url], true).await;
        assert!(third.is_some());
        assert_eq!(attempts.last().unwrap().result, AttemptResult::NotModified);
        let seen = seen.lock().unwrap();
        assert_eq!(seen.requests.len(), 2);
        assert_eq!(
            seen.requests[1].1.get("if-none-match").map(String::as_str),
            Some("\"v1\"")
        );
    }

    #[tokio::test]
    async fn not_found_is_remembered_and_fallback_urls_are_tried() {
        let mut routes = HashMap::new();
        routes.insert(
            "/raw/Named_Boxarts/Game%20%28USA%29.png".to_owned(),
            Reply {
                status: 200,
                headers: vec![("Content-Type", "image/png".into())],
                body: png(64, 90),
            },
        );
        let (base, seen) = serve(routes).await;
        let (_dir, service) = service();
        let urls = vec![
            format!("{base}/thumbs/Named_Boxarts/Game%20%28USA%29.png"),
            format!("{base}/raw/Named_Boxarts/Game%20%28USA%29.png"),
        ];
        let (cover, attempts) =
            fetch_url(&service, SourceId::Libretro, "Game (USA)", urls, false).await;
        assert!(cover.is_some());
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].result, AttemptResult::Missing);
        assert_eq!(attempts[1].result, AttemptResult::Downloaded);

        let missing = vec![format!("{base}/none.png"), format!("{base}/none2.png")];
        let (cover, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "SLUS-99999",
            missing.clone(),
            false,
        )
        .await;
        assert!(cover.is_none());
        assert_eq!(attempts.len(), 2);
        let before = seen.lock().unwrap().requests.len();
        // The negative entry stops further requests for a week.
        let (cover, attempts) =
            fetch_url(&service, SourceId::Xlenore, "SLUS-99999", missing, false).await;
        assert!(cover.is_none());
        assert_eq!(attempts.last().unwrap().result, AttemptResult::Missing);
        assert_eq!(seen.lock().unwrap().requests.len(), before);
    }

    #[tokio::test]
    async fn requests_to_one_source_are_a_second_apart() {
        let mut routes = HashMap::new();
        for n in 0..3 {
            routes.insert(
                format!("/c{n}.jpg"),
                Reply {
                    status: 200,
                    headers: vec![("Content-Type", "image/jpeg".into())],
                    body: jpeg(8, 8),
                },
            );
        }
        let (base, seen) = serve(routes).await;
        let (_dir, service) = service();
        let started = Instant::now();
        for n in 0..3 {
            let (cover, _) = fetch_url(
                &service,
                SourceId::Xlenore,
                &format!("K{n}"),
                vec![format!("{base}/c{n}.jpg")],
                false,
            )
            .await;
            assert!(cover.is_some());
        }
        let times = seen.lock().unwrap().times.clone();
        assert_eq!(times.len(), 3);
        assert!(times[1].duration_since(times[0]) >= std::time::Duration::from_millis(900));
        assert!(times[2].duration_since(times[1]) >= std::time::Duration::from_millis(900));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn server_errors_back_off_then_give_up_and_junk_is_refused() {
        let mut routes = HashMap::new();
        routes.insert(
            "/busy.jpg".to_owned(),
            Reply {
                status: 503,
                headers: vec![("Retry-After", "1".into())],
                body: Vec::new(),
            },
        );
        routes.insert(
            "/html.jpg".to_owned(),
            Reply {
                status: 200,
                headers: vec![("Content-Type", "text/html".into())],
                body: b"<html>not a cover</html>".to_vec(),
            },
        );
        let (base, seen) = serve(routes).await;
        let (_dir, service) = service();
        let (cover, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "BUSY",
            vec![format!("{base}/busy.jpg")],
            false,
        )
        .await;
        assert!(cover.is_none());
        assert!(
            matches!(attempts[0].result, AttemptResult::Error(_)),
            "{attempts:?}"
        );
        assert_eq!(
            seen.lock().unwrap().requests.len(),
            4,
            "one try plus three retries"
        );

        let (cover, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "HTML",
            vec![format!("{base}/html.jpg")],
            false,
        )
        .await;
        assert!(cover.is_none());
        assert!(
            matches!(&attempts[0].result, AttemptResult::Error(e) if e.contains("not an image"))
        );
        assert!(service.cache().get("xlenore", "HTML").unwrap().is_none());
    }

    #[tokio::test]
    async fn mirrors_and_local_files_need_no_network() {
        let (_dir, service) = service();
        let mirror = tempfile::tempdir().unwrap();
        let covers = mirror.path().join("covers").join("default");
        std::fs::create_dir_all(&covers).unwrap();
        std::fs::write(covers.join("SLUS-20062.jpg"), jpeg(200, 300)).unwrap();
        let thumbs = tempfile::tempdir().unwrap();
        let boxarts = thumbs.path().join("Named_Boxarts");
        std::fs::create_dir_all(&boxarts).unwrap();
        std::fs::write(
            boxarts.join("Dead or Alive 2 (Europe) (En,Fr,De,Es,It).png"),
            png(50, 70),
        )
        .unwrap();

        let sources = [
            SourceConfig::mirror(SourceId::Xlenore, mirror.path()),
            SourceConfig::mirror(SourceId::Libretro, thumbs.path()),
        ];
        let names = vec!["Dead or Alive 2 (Europe) (En,Fr,De,Es,It)".to_owned()];
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial: "SLUS-20062",
                redump_names: &[],
                size: SIZE,
                sources: &sources,
                revalidate: false,
            })
            .await;
        let cover = outcome.cover.unwrap();
        assert_eq!(cover.source, "xlenore");
        assert_eq!(outcome.attempts.len(), 1);

        // xlenore has nothing: libretro by name.
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial: "SCES-50003",
                redump_names: &names,
                size: SIZE,
                sources: &sources,
                revalidate: false,
            })
            .await;
        let cover = outcome.cover.unwrap();
        assert_eq!(cover.source, "libretro");
        assert_eq!(outcome.attempts[0].result, AttemptResult::Missing);
        assert_eq!(outcome.attempts[1].result, AttemptResult::Downloaded);
        let header = png_header(&std::fs::read(&cover.png).unwrap()).unwrap();
        assert_eq!((header.width, header.height), (140, 200));

        // Nothing anywhere.
        let outcome = service
            .fetch_cover(&CoverRequest {
                serial: "SLUS-00000",
                redump_names: &[],
                size: SIZE,
                sources: &sources,
                revalidate: false,
            })
            .await;
        assert!(outcome.cover.is_none());
        assert_eq!(
            outcome.attempts.len(),
            1,
            "no names: libretro is not even tried"
        );

        // A user-supplied file.
        let local = service
            .import_local(&png(30, 30), SIZE, "C:\\covers\\mine.png")
            .unwrap();
        assert_eq!(local.source, LOCAL_SOURCE);
        assert_eq!(local.origin, "C:\\covers\\mine.png");
        assert!(local.png.is_file());
        assert!(service.import_local(b"junk", SIZE, "x").is_err());
    }
}
