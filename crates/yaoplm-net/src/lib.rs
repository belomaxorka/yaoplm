//! Picture sources, the download cache, request limits and PNG
//! transcoding (ART-1, ART-2, ADR-0007, ADR-0035), and the Redump DAT for
//! dump verification (META-2).
//!
//! Asynchronous on purpose: the core never uses this crate (ADR-0003);
//! `yaoplm-api` drives [`ArtService`] from a job thread through a tokio
//! runtime. Network access happens only inside [`ArtService::fetch_cover`],
//! [`ArtService::find_variants`] and [`redump::RedumpStore::update`], each
//! started by a user action, and every answer is cached (ARCHITECTURE.md
//! §6-§7).

pub mod cache;
pub mod http;
pub mod oplm;
pub mod redump;
pub mod region;
pub mod remote_zip;
pub mod screenscraper;
pub mod sources;
pub mod transcode;

#[cfg(test)]
mod remote_tests;
#[cfg(test)]
mod test_server;

use std::path::{Path, PathBuf};

use transcode::Fit;
use yaoplm_formats::art::{ArtKind, ArtSize};
use yaoplm_formats::serial;

use cache::{Cache, Download, Record, Status};
use http::{Fetched, Http};
use oplm::Oplm;
use screenscraper::ScreenScraper;
use sources::{Candidate, Location, SourceConfig, SourceId};

/// At most this many Redump names are tried per game: each is a request.
pub const MAX_LIBRETRO_CANDIDATES: usize = 3;
/// At most this many other releases stand in for a game, with their first
/// Redump name each.
pub const MAX_OTHER_RELEASES: usize = 2;
/// The pseudo-source of covers the user supplied (file, drop, paste).
pub const LOCAL_SOURCE: &str = "local";

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("{0}")]
    Http(String),
    #[error("{url}: HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{0}: the response is larger than expected")]
    TooLarge(String),
    #[error("image: {0}")]
    Image(String),
    #[error("cache: {0}")]
    Cache(String),
    #[error("zip: {0}")]
    Zip(String),
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

/// One game's picture to find.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverRequest<'a> {
    /// `SLUS-20062`.
    pub serial: &'a str,
    /// Redump names in the order worth trying (region first).
    pub redump_names: &'a [String],
    pub kind: ArtKind,
    /// The profile's size for `kind`.
    pub size: ArtSize,
    /// The sources that are on; an empty list means nothing is asked. They
    /// are tried in the order of [`sources::order`], not in this one.
    pub sources: &'a [SourceConfig],
    /// Ask the server even when the cache has an answer (conditional
    /// request); "overwrite" runs set this.
    pub revalidate: bool,
    /// The game's releases in other regions (the same Redump title). For
    /// types shared across markets they are asked when the game's own
    /// serial has nothing, the way RetroArch falls back to the short name.
    pub others: &'a [OtherRelease],
}

/// Another release of the same game (ADR-0035).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherRelease {
    /// `SCES-50003`.
    pub serial: String,
    pub redump_names: Vec<String>,
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
    /// The source has a picture, but it shows another market's release
    /// (ADR-0035): left out rather than put on the wrong game.
    OtherMarket,
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

/// A picture found at a source; one that may show another market's
/// release is offered only when the user picks by hand.
struct Found {
    cover: Cover,
    other_market: bool,
}

/// Where the remote sources are. Tests point them at a local server.
#[derive(Debug, Clone, Default)]
pub struct Endpoints {
    pub oplm: oplm::Archive,
    pub screenscraper: screenscraper::Api,
}

/// Finds covers and keeps them in the cache.
#[derive(Debug)]
pub struct ArtService {
    http: Http,
    cache: Cache,
    oplm: Oplm,
    screenscraper: ScreenScraper,
}

fn attempt(source: SourceId, target: impl Into<String>, result: AttemptResult) -> Attempt {
    Attempt {
        source: source.as_str().to_owned(),
        target: target.into(),
        result,
    }
}

impl ArtService {
    pub fn open(data_dir: &Path) -> Result<Self, NetError> {
        Self::open_with(data_dir, Endpoints::default())
    }

    pub fn open_with(data_dir: &Path, endpoints: Endpoints) -> Result<Self, NetError> {
        let cache = Cache::open(data_dir)?;
        let art_dir = cache.root().join(cache::ART_DIR);
        Ok(Self {
            http: Http::new()?,
            oplm: Oplm::new(endpoints.oplm, &art_dir),
            screenscraper: ScreenScraper::new(endpoints.screenscraper),
            cache,
        })
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// The shared client, for other downloads (the Redump DAT).
    pub fn http(&self) -> &Http {
        &self.http
    }

    /// Whether ScreenScraper can be asked: this build or the environment
    /// has developer credentials.
    pub fn screenscraper_ready(&self) -> bool {
        self.screenscraper.api.dev.is_some()
    }

    /// `COV-140x200`: the kind decides how the picture is fitted.
    fn derived_label(kind: ArtKind, size: ArtSize) -> String {
        format!("{}-{}x{}", kind.suffix(), size.width, size.height)
    }

    fn cover_from(
        &self,
        record: &Record,
        kind: ArtKind,
        size: ArtSize,
        from_cache: bool,
        origin: &str,
    ) -> Result<Cover, NetError> {
        let png = self
            .cache
            .derived(record, &Self::derived_label(kind, size), |bytes| {
                transcode::to_png_fit(bytes, size, Fit::for_kind(kind)).map(|t| t.png)
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

    fn candidates(request: &CoverRequest<'_>, source: SourceId, variants: bool) -> Vec<Candidate> {
        let names: Vec<String> = request
            .redump_names
            .iter()
            .take(MAX_LIBRETRO_CANDIDATES)
            .cloned()
            .collect();
        sources::candidates(source, request.kind, request.serial, &names, variants)
    }

    async fn try_candidate(
        &self,
        config: &SourceConfig,
        candidate: &Candidate,
        request: &CoverRequest<'_>,
        attempts: &mut Vec<Attempt>,
    ) -> Option<Cover> {
        match &config.location {
            Location::Mirror(dir) => self.via_mirror(
                config.id,
                candidate,
                dir,
                request.kind,
                request.size,
                attempts,
            ),
            Location::Remote => {
                self.via_remote(
                    config.id,
                    candidate,
                    request.kind,
                    request.size,
                    request.revalidate,
                    attempts,
                )
                .await
            }
        }
    }

    /// What one source has for the request: the first fitting picture, or
    /// with `variants` every picture to pick from.
    async fn ask_source(
        &self,
        config: &SourceConfig,
        request: &CoverRequest<'_>,
        variants: bool,
        attempts: &mut Vec<Attempt>,
    ) -> Vec<Found> {
        match (config.id, &config.location) {
            (SourceId::Oplm, Location::Remote) => {
                return self.via_oplm(request, variants, attempts).await;
            }
            (SourceId::ScreenScraper, _) => {
                return self
                    .via_screenscraper(config, request, variants, attempts)
                    .await;
            }
            _ => {}
        }
        let candidates = Self::candidates(request, config.id, variants);
        let mut found = Vec::new();
        for candidate in &candidates {
            if let Some(cover) = self
                .try_candidate(config, candidate, request, attempts)
                .await
            {
                found.push(Found {
                    cover,
                    other_market: false,
                });
                if !variants {
                    break;
                }
            }
        }
        found
    }

    /// Tries the sources in the order of their type and returns the first
    /// picture found, with every attempt made along the way.
    pub async fn fetch_cover(&self, request: &CoverRequest<'_>) -> CoverOutcome {
        let mut attempts = Vec::new();
        for (request, config) in Self::plan(request) {
            if let Some(found) = self
                .ask_source(config, &request, false, &mut attempts)
                .await
                .into_iter()
                .next()
            {
                return CoverOutcome {
                    cover: Some(found.cover),
                    attempts,
                };
            }
        }
        CoverOutcome {
            cover: None,
            attempts,
        }
    }

    /// The game's own serial at every source in the order of its type,
    /// then, for a type shared across markets, its other releases at every
    /// source but ScreenScraper (which already answered for every market).
    fn plan<'a>(request: &'a CoverRequest<'a>) -> Vec<(CoverRequest<'a>, &'a SourceConfig)> {
        let ordered = sources::ordered(request.sources, request.kind, request.size);
        let mut plan: Vec<(CoverRequest<'a>, &'a SourceConfig)> =
            ordered.iter().map(|c| (request.clone(), *c)).collect();
        if region::region_specific(request.kind) {
            return plan;
        }
        for other in request.others.iter().take(MAX_OTHER_RELEASES) {
            let names = &other.redump_names[..other.redump_names.len().min(1)];
            for config in ordered.iter().filter(|c| c.id != SourceId::ScreenScraper) {
                plan.push((
                    CoverRequest {
                        serial: &other.serial,
                        redump_names: names,
                        others: &[],
                        ..request.clone()
                    },
                    *config,
                ));
            }
        }
        plan
    }

    /// Every picture the sources have for the request, in source order,
    /// with the alternatives (3-D covers, more screenshots): what a user
    /// picks from (SPEC §5.4.1). Pictures that may show another market's
    /// release come last. The game's other releases are asked only when its
    /// own serial has nothing, as in [`Self::fetch_cover`]: each of their
    /// pictures is another request to wait for. A source answers once per
    /// place, a second apart.
    pub async fn find_variants(&self, request: &CoverRequest<'_>) -> (Vec<Cover>, Vec<Attempt>) {
        let mut attempts = Vec::new();
        let mut found: Vec<Cover> = Vec::new();
        let mut last: Vec<Cover> = Vec::new();
        let own = sources::ordered(request.sources, request.kind, request.size).len();
        for (index, (request, config)) in Self::plan(request).into_iter().enumerate() {
            if index == own && !(found.is_empty() && last.is_empty()) {
                break;
            }
            for picture in self.ask_source(config, &request, true, &mut attempts).await {
                // Several Redump names often lead to the same file.
                let seen = found
                    .iter()
                    .chain(last.iter())
                    .any(|c| same_picture(&c.png, &picture.cover.png));
                if !seen {
                    if picture.other_market {
                        last.push(picture.cover);
                    } else {
                        found.push(picture.cover);
                    }
                }
            }
        }
        found.extend(last);
        (found, attempts)
    }

    fn via_mirror(
        &self,
        source: SourceId,
        candidate: &Candidate,
        dir: &Path,
        kind: ArtKind,
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
            &mirror_key(dir, &candidate.key),
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
        match self.cover_from(&record, kind, size, false, &target) {
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

    /// A cached original of `source`/`key` that is still on disk.
    fn cached(&self, source: SourceId, key: &str) -> Result<Option<Record>, NetError> {
        Ok(self
            .cache
            .get(source.as_str(), key)?
            .filter(|r| r.status == Status::Ok && self.cache.original_path(r).is_some()))
    }

    /// Stores downloaded bytes and derives the slot's PNG.
    fn store(
        &self,
        source: SourceId,
        key: &str,
        url: &str,
        bytes: &[u8],
        request: &CoverRequest<'_>,
    ) -> Result<Cover, NetError> {
        let info = source.info();
        let record = self.cache.put(
            source.as_str(),
            key,
            &Download {
                url,
                bytes,
                content_type: None,
                etag: None,
                last_modified: None,
                license_tag: info.license_tag,
                attribution: info.attribution,
            },
        )?;
        self.cover_from(&record, request.kind, request.size, false, url)
    }

    /// OPL Manager's backup through its index (ADR-0035): no request for a
    /// picture the backup does not have, and none for one that shows
    /// another market's release unless the user picks by hand.
    async fn via_oplm(
        &self,
        request: &CoverRequest<'_>,
        variants: bool,
        attempts: &mut Vec<Attempt>,
    ) -> Vec<Found> {
        let source = SourceId::Oplm;
        let archive = self.oplm.archive();
        let id = serial::serial_to_opl(request.serial);
        let index = match self.oplm.index(&self.http).await {
            Ok(index) => index,
            Err(err) => {
                let target = archive.urls.first().cloned().unwrap_or_default();
                attempts.push(attempt(
                    source,
                    target,
                    AttemptResult::Error(err.to_string()),
                ));
                return Vec::new();
            }
        };
        let pictures = index.pictures(&id, request.kind, variants);
        if pictures.is_empty() {
            let wanted = format!("PS2/{id}/{id}_{}.png", request.kind.suffix());
            attempts.push(attempt(
                source,
                archive.member_url(&wanted),
                AttemptResult::Missing,
            ));
            return Vec::new();
        }
        let packaging = region::region_specific(request.kind);
        let mut found = Vec::new();
        for picture in pictures {
            let url = archive.member_url(&picture.member.name);
            let other_market = packaging && picture.shared_across_markets;
            if other_market && !variants {
                attempts.push(attempt(source, url, AttemptResult::OtherMarket));
                continue;
            }
            let key = picture.key();
            let cover = match self.cached(source, &key) {
                Ok(Some(record)) => self
                    .cover_from(&record, request.kind, request.size, true, &url)
                    .map(|c| (c, AttemptResult::Cached)),
                Ok(None) => match self.oplm.read(&self.http, picture).await {
                    Ok(bytes) => self
                        .store(source, &key, &url, &bytes, request)
                        .map(|c| (c, AttemptResult::Downloaded)),
                    Err(err) => Err(err),
                },
                Err(err) => Err(err),
            };
            match cover {
                Ok((cover, result)) => {
                    attempts.push(attempt(source, url, result));
                    found.push(Found {
                        cover,
                        other_market,
                    });
                    if !variants {
                        break;
                    }
                }
                Err(err) => {
                    attempts.push(attempt(source, url, AttemptResult::Error(err.to_string())))
                }
            }
        }
        found
    }

    /// ScreenScraper: the game's media list, then the media that fit the
    /// slot and the game's market.
    async fn via_screenscraper(
        &self,
        config: &SourceConfig,
        request: &CoverRequest<'_>,
        variants: bool,
        attempts: &mut Vec<Attempt>,
    ) -> Vec<Found> {
        let source = SourceId::ScreenScraper;
        let ss = &self.screenscraper;
        let account = config
            .account
            .clone()
            .or_else(screenscraper::account_from_env);
        let Some(info_url) = ss.api.info_url(account.as_ref(), request.serial) else {
            // No developer key: the source is not set up.
            return Vec::new();
        };
        let shown = screenscraper::redact(&info_url);
        if let Some(why) = ss.paused() {
            attempts.push(attempt(source, shown, AttemptResult::Error(why)));
            return Vec::new();
        }
        let info = match self.screenscraper_info(request, &info_url, &shown).await {
            Ok(Some(info)) => info,
            Ok(None) => {
                attempts.push(attempt(source, shown, AttemptResult::Missing));
                return Vec::new();
            }
            Err(err) => {
                attempts.push(attempt(source, shown, AttemptResult::Error(err)));
                return Vec::new();
            }
        };
        let medias = screenscraper::pick(&info.medias, request.kind, request.serial, variants);
        if medias.is_empty() {
            let any = info
                .medias
                .iter()
                .any(|m| screenscraper::media_types(request.kind).contains(&m.kind.as_str()));
            attempts.push(attempt(
                source,
                shown,
                if any {
                    AttemptResult::OtherMarket
                } else {
                    AttemptResult::Missing
                },
            ));
            return Vec::new();
        }
        let mut found = Vec::new();
        for media in medias {
            let key = format!("{}({})/{}", media.kind, media.region, request.serial);
            let url = screenscraper::redact(&media.url);
            let result = match self.cached(source, &key) {
                Ok(Some(record)) => self
                    .cover_from(&record, request.kind, request.size, true, &url)
                    .map(|c| Some((c, AttemptResult::Cached)))
                    .map_err(|e| e.to_string()),
                Ok(None) => match self
                    .http
                    .get(screenscraper::LIMITER, &media.url, &Default::default())
                    .await
                {
                    Ok(Fetched::Ok(body)) if transcode::looks_like_image(&body.bytes) => self
                        .store(source, &key, &url, &body.bytes, request)
                        .map(|c| Some((c, AttemptResult::Downloaded)))
                        .map_err(|e| e.to_string()),
                    // `NOMEDIA`, a 404, or an answer that is not a picture.
                    Ok(_) => Ok(None),
                    Err(err) => Err(self.screenscraper_error(&err)),
                },
                Err(err) => Err(err.to_string()),
            };
            match result {
                Ok(Some((cover, result))) => {
                    attempts.push(attempt(source, url, result));
                    found.push(Found {
                        cover,
                        other_market: false,
                    });
                    if !variants {
                        break;
                    }
                }
                Ok(None) => attempts.push(attempt(source, url, AttemptResult::Missing)),
                Err(err) => {
                    attempts.push(attempt(source, url, AttemptResult::Error(err)));
                    if ss.paused().is_some() {
                        break;
                    }
                }
            }
        }
        found
    }

    /// A game's media list from the cache (30 days, "not found" for 7) or
    /// the API. `None`: ScreenScraper does not know the serial.
    async fn screenscraper_info(
        &self,
        request: &CoverRequest<'_>,
        url: &str,
        shown: &str,
    ) -> Result<Option<screenscraper::GameInfo>, String> {
        let source = SourceId::ScreenScraper;
        let key = format!("jeu/{}", request.serial);
        let now = cache::now_secs();
        let record = self
            .cache
            .get(source.as_str(), &key)
            .map_err(|e| e.to_string())?;
        if let Some(record) = &record {
            let fresh = now.saturating_sub(record.fetched_at) < screenscraper::INFO_TTL.as_secs();
            match record.status {
                Status::Ok if fresh && !request.revalidate => {
                    if let Some(bytes) = self
                        .cache
                        .original_path(record)
                        .and_then(|p| std::fs::read(p).ok())
                        && let Ok(info) = screenscraper::parse_game_info(&bytes)
                    {
                        return Ok(Some(info));
                    }
                }
                Status::Missing if !record.is_stale_missing(now) => return Ok(None),
                _ => {}
            }
        }
        let body = match self
            .http
            .get(screenscraper::LIMITER, url, &Default::default())
            .await
        {
            Ok(Fetched::Ok(body)) => body,
            Ok(_) => {
                let _ = self.cache.put_missing(source.as_str(), &key, shown);
                return Ok(None);
            }
            Err(err) => return Err(self.screenscraper_error(&err)),
        };
        let info = screenscraper::parse_game_info(&body.bytes).map_err(|e| e.to_string())?;
        if info.quota_spent() {
            let (why, pause) = screenscraper::status_pause(430);
            self.screenscraper.pause(&why, pause.unwrap_or_default());
        }
        let info_tag = source.info();
        self.cache
            .put(
                source.as_str(),
                &key,
                &Download {
                    url: shown,
                    bytes: &body.bytes,
                    content_type: Some("application/xml"),
                    etag: None,
                    last_modified: None,
                    license_tag: info_tag.license_tag,
                    attribution: info_tag.attribution,
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(Some(info))
    }

    /// The message of a failed ScreenScraper request, with credentials
    /// removed, and a pause when retrying soon would not help.
    fn screenscraper_error(&self, err: &NetError) -> String {
        match err {
            NetError::Status { status, .. } => {
                let (why, pause) = screenscraper::status_pause(*status);
                if let Some(pause) = pause {
                    self.screenscraper.pause(&why, pause);
                }
                why
            }
            other => redact_all(&other.to_string()),
        }
    }

    async fn via_remote(
        &self,
        source: SourceId,
        candidate: &Candidate,
        kind: ArtKind,
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
                    let result = self.cover_from(record, kind, size, true, &record.url);
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
                    Some(Ok(fresh)) => match self.cover_from(&fresh, kind, size, true, url) {
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
                        match stored.and_then(|r| self.cover_from(&r, kind, size, false, url)) {
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

    /// Stores a picture the user supplied (ART-3) and derives the PNG of
    /// `kind` at `size`. `label` is kept as the origin for the log.
    pub fn import_local(
        &self,
        bytes: &[u8],
        kind: ArtKind,
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
        self.cover_from(&record, kind, size, false, label)
    }
}

/// The cache key of a picture read from the mirror in `dir`: apart from
/// the key the network uses, so a mirror's file (a test fixture, a local
/// copy) never answers for the source on the network, and two mirrors never
/// answer for each other.
fn mirror_key(dir: &Path, key: &str) -> String {
    let folder = cache::sha256_hex(dir.to_string_lossy().as_bytes());
    format!("mirror/{}/{key}", &folder[..12])
}

/// `text` with ScreenScraper credentials removed from any URL in it.
fn redact_all(text: &str) -> String {
    screenscraper::redact_text(text)
}

/// Two PNGs with the same bytes. Files that cannot be read are never the
/// same: two failed reads say nothing about the pictures.
fn same_picture(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Instant;

    use image::{DynamicImage, ImageBuffer, Rgb};
    use yaoplm_formats::art::png_header;

    use super::*;
    use crate::test_server::{Reply, serve};

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
            .via_remote(
                source,
                &candidate,
                ArtKind::Cov,
                SIZE,
                revalidate,
                &mut attempts,
            )
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
                failures: 0,
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
                failures: 0,
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
                    failures: 0,
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
        assert_eq!(seen.lock().unwrap().times.len(), 3);
        // A second before each request after the first, timed here: under
        // load a request can reach the mock server late, which shrinks the
        // gap the server would see.
        assert!(started.elapsed() >= std::time::Duration::from_millis(1950));
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
                failures: 0,
            },
        );
        routes.insert(
            "/html.jpg".to_owned(),
            Reply {
                status: 200,
                headers: vec![("Content-Type", "text/html".into())],
                body: b"<html>not a cover</html>".to_vec(),
                failures: 0,
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
                kind: ArtKind::Cov,
                serial: "SLUS-20062",
                redump_names: &[],
                size: SIZE,
                sources: &sources,
                revalidate: false,
                others: &[],
            })
            .await;
        let cover = outcome.cover.unwrap();
        assert_eq!(cover.source, "xlenore");
        assert_eq!(outcome.attempts.len(), 1);

        // xlenore has nothing: libretro by name.
        let outcome = service
            .fetch_cover(&CoverRequest {
                kind: ArtKind::Cov,
                serial: "SCES-50003",
                redump_names: &names,
                size: SIZE,
                sources: &sources,
                revalidate: false,
                others: &[],
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
                kind: ArtKind::Cov,
                serial: "SLUS-00000",
                redump_names: &[],
                size: SIZE,
                sources: &sources,
                revalidate: false,
                others: &[],
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
            .import_local(&png(30, 30), ArtKind::Cov, SIZE, "C:\\covers\\mine.png")
            .unwrap();
        assert_eq!(local.source, LOCAL_SOURCE);
        assert_eq!(local.origin, "C:\\covers\\mine.png");
        assert!(local.png.is_file());
        assert!(
            service
                .import_local(b"junk", ArtKind::Cov, SIZE, "x")
                .is_err()
        );
    }

    /// The walkthrough of 2026-10-24: a cover once read from a test mirror
    /// was served from the cache as xlenore's cover on the network.
    #[tokio::test]
    async fn a_mirror_never_answers_for_the_network() {
        let mut routes = HashMap::new();
        routes.insert(
            "/covers/default/SLUS-20062.jpg".to_owned(),
            Reply::ok("image/jpeg", jpeg(300, 420)),
        );
        let (base, seen) = serve(routes).await;
        let (_dir, service) = service();
        let mirror = tempfile::tempdir().unwrap();
        let covers = mirror.path().join("covers").join("default");
        std::fs::create_dir_all(&covers).unwrap();
        std::fs::write(covers.join("SLUS-20062.jpg"), jpeg(200, 300)).unwrap();
        let sources = [SourceConfig::mirror(SourceId::Xlenore, mirror.path())];
        let from_mirror = service
            .fetch_cover(&CoverRequest {
                kind: ArtKind::Cov,
                serial: "SLUS-20062",
                redump_names: &[],
                size: SIZE,
                sources: &sources,
                revalidate: false,
                others: &[],
            })
            .await
            .cover
            .unwrap();
        assert!(
            from_mirror.key.starts_with("mirror/"),
            "{}",
            from_mirror.key
        );
        // The same cover on the network is asked for, not taken from the
        // mirror's copy in the cache.
        let (cover, attempts) = fetch_url(
            &service,
            SourceId::Xlenore,
            "SLUS-20062",
            vec![format!("{base}/covers/default/SLUS-20062.jpg")],
            false,
        )
        .await;
        assert_eq!(attempts.last().unwrap().result, AttemptResult::Downloaded);
        assert_ne!(
            std::fs::read(cover.unwrap().png).unwrap(),
            std::fs::read(&from_mirror.png).unwrap()
        );
        assert_eq!(seen.lock().unwrap().requests.len(), 1);
    }

    #[tokio::test]
    async fn variants_and_kinds_from_mirrors() {
        let (_dir, service) = service();
        let mirror = tempfile::tempdir().unwrap();
        for (path, bytes) in [
            ("covers/default/SLUS-20062.jpg", jpeg(200, 300)),
            ("covers/3d/SLUS-20062.png", png(220, 300)),
        ] {
            let path = mirror.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let thumbs = tempfile::tempdir().unwrap();
        for (folder, bytes) in [
            ("Named_Boxarts", png(50, 70)),
            ("Named_Snaps", png(64, 48)),
            ("Named_Logos", png(90, 30)),
        ] {
            let dir = thumbs.path().join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Retail (USA).png"), bytes).unwrap();
        }
        let sources = [
            SourceConfig::mirror(SourceId::Xlenore, mirror.path()),
            SourceConfig::mirror(SourceId::Libretro, thumbs.path()),
        ];
        let names = vec!["Retail (USA)".to_owned()];
        let request = |kind, size| CoverRequest {
            serial: "SLUS-20062",
            redump_names: &names,
            kind,
            size,
            sources: &sources,
            revalidate: false,
            others: &[],
        };
        // Front covers: xlenore's 2-D and 3-D, then libretro's box art.
        let (covers, _) = service.find_variants(&request(ArtKind::Cov, SIZE)).await;
        let origins: Vec<&str> = covers.iter().map(|c| c.origin.as_str()).collect();
        assert_eq!(covers.len(), 3, "{origins:?}");
        assert!(origins[0].ends_with("SLUS-20062.jpg"));
        assert!(origins[1].ends_with("SLUS-20062.png"));
        assert_eq!(covers[2].source, "libretro");

        // A logo keeps its shape in its size; snaps come from their folder.
        let logo_size = ArtSize {
            width: 300,
            height: 125,
        };
        let logo = service
            .fetch_cover(&request(ArtKind::Lgo, logo_size))
            .await
            .cover
            .unwrap();
        assert!(logo.origin.contains("Named_Logos"));
        let header = png_header(&std::fs::read(&logo.png).unwrap()).unwrap();
        assert_eq!(
            (header.width, header.height, header.color_type),
            (300, 125, 6)
        );
        let snap_size = ArtSize {
            width: 250,
            height: 188,
        };
        let snap = service
            .fetch_cover(&request(ArtKind::Scr, snap_size))
            .await
            .cover
            .unwrap();
        assert!(snap.origin.contains("Named_Snaps"));
        // The box art the COV search cached is still there for COV.
        let again = service.fetch_cover(&request(ArtKind::Cov, SIZE)).await;
        assert_eq!(again.cover.unwrap().source, "xlenore");
        // No source has icons.
        let none = service.fetch_cover(&request(ArtKind::Ico, SIZE)).await;
        assert!(none.cover.is_none() && none.attempts.is_empty());
    }

    #[test]
    fn pictures_are_the_same_only_when_both_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a.png"),
            dir.path().join("b.png"),
            dir.path().join("c.png"),
        );
        std::fs::write(&a, png(4, 4)).unwrap();
        std::fs::write(&b, png(4, 4)).unwrap();
        std::fs::write(&c, png(5, 4)).unwrap();
        assert!(same_picture(&a, &b));
        assert!(!same_picture(&a, &c));
        let (gone, lost) = (dir.path().join("gone.png"), dir.path().join("lost.png"));
        assert!(!same_picture(&gone, &lost));
        assert!(!same_picture(&a, &gone));
    }

    #[tokio::test]
    async fn other_releases_stand_in_for_shared_types_only() {
        let (_dir, service) = service();
        let thumbs = tempfile::tempdir().unwrap();
        for folder in ["Named_Snaps", "Named_Boxarts"] {
            let dir = thumbs.path().join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("Game (Europe).png"), png(64, 48)).unwrap();
        }
        let sources = [SourceConfig::mirror(SourceId::Libretro, thumbs.path())];
        let names = vec!["Game (Japan)".to_owned()];
        let others = vec![OtherRelease {
            serial: "SLES-50001".into(),
            redump_names: vec!["Game (Europe)".into(), "Game (Europe) (Rev 1)".into()],
        }];
        let request = |kind| CoverRequest {
            serial: "SLPS-25001",
            redump_names: &names,
            kind,
            size: SIZE,
            sources: &sources,
            revalidate: false,
            others: &others,
        };
        // The Japanese release has no screenshot: the European one has.
        let snap = service.fetch_cover(&request(ArtKind::Scr)).await;
        let cover = snap.cover.unwrap();
        assert!(
            cover.origin.ends_with("Game (Europe).png"),
            "{}",
            cover.origin
        );
        assert_eq!(snap.attempts.len(), 2, "{:?}", snap.attempts);
        // A box is the release itself: never another region's.
        let cover = service.fetch_cover(&request(ArtKind::Cov)).await;
        assert!(cover.cover.is_none());
        assert!(cover.attempts.iter().all(|a| !a.target.contains("Europe")));
        // To pick from, too, the other release stands in only for nothing.
        let (variants, _) = service.find_variants(&request(ArtKind::Scr)).await;
        assert_eq!(variants.len(), 1);
        assert!(variants[0].origin.ends_with("Game (Europe).png"));
        std::fs::write(
            thumbs.path().join("Named_Snaps").join("Game (Japan).png"),
            png(48, 64),
        )
        .unwrap();
        let (variants, attempts) = service.find_variants(&request(ArtKind::Scr)).await;
        assert_eq!(variants.len(), 1);
        assert!(variants[0].origin.ends_with("Game (Japan).png"));
        assert!(
            attempts.iter().all(|a| !a.target.contains("Europe")),
            "{attempts:?}"
        );
    }
}
