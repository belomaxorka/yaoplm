//! The one HTTP client every source shares (ADR-0007, research/03 §7.2).
//!
//! - User-Agent `YAOPLM/<version> (+https://github.com/belomaxorka/yaoplm)`;
//! - at most one request per second per source (a token bucket of one);
//! - conditional requests with `If-None-Match` / `If-Modified-Since`;
//! - exponential back-off on 429 and 5xx, honouring `Retry-After`;
//! - a size cap, so a wrong URL cannot pull gigabytes into memory;
//! - range requests, to read single members of a zip on archive.org;
//! - `YAOPLM_OFFLINE=1` refuses every host but this machine's, which CI
//!   sets so that a test that forgets a mirror fails instead of going out.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::header::{self, HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use tokio::time::Instant;

use crate::NetError;

pub const USER_AGENT: &str = concat!(
    "YAOPLM/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/belomaxorka/yaoplm)"
);
/// Covers are a few hundred kilobytes; anything near this is not one.
pub const MAX_BODY: usize = 16 << 20;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MIN_INTERVAL: Duration = Duration::from_secs(1);
const RETRIES: u32 = 3;
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// What the cache already knows about a URL, for a conditional request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Body {
    pub bytes: Vec<u8>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// The `filename` of `Content-Disposition`, without any path.
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Ok(Body),
    /// 304: what the cache has is still current.
    NotModified,
    /// 404 or 410.
    NotFound,
}

/// Which bytes of a file to ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRange {
    /// `len` bytes from `start`.
    Span { start: u64, len: u64 },
    /// The last `len` bytes.
    Suffix(u64),
}

/// Part of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranged {
    pub bytes: Vec<u8>,
    /// Where `bytes` start in the file.
    pub start: u64,
    /// The size of the whole file.
    pub total: u64,
    pub etag: Option<String>,
}

/// A shared client with a per-source request limiter.
pub struct Http {
    client: Client,
    /// Earliest start of the next request, per limiter key (the source id).
    next: Mutex<HashMap<String, Instant>>,
}

impl std::fmt::Debug for Http {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Http").finish_non_exhaustive()
    }
}

impl Http {
    pub fn new() -> Result<Self, NetError> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|e| NetError::Http(e.to_string()))?;
        Ok(Self {
            client,
            next: Mutex::new(HashMap::new()),
        })
    }

    /// Waits for the limiter of `key`: requests to one source are at least
    /// a second apart, whatever thread asks.
    async fn throttle(&self, key: &str) {
        let wait = {
            let mut next = self
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let now = Instant::now();
            let at = next.get(key).copied().unwrap_or(now).max(now);
            next.insert(key.to_owned(), at + MIN_INTERVAL);
            at.saturating_duration_since(now)
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }

    /// GETs `url`, retrying on 429 and 5xx with back-off. `limiter` names
    /// the source the request counts against.
    pub async fn get(
        &self,
        limiter: &str,
        url: &str,
        validators: &Validators,
    ) -> Result<Fetched, NetError> {
        self.get_large(limiter, url, validators, MAX_BODY as u64)
            .await
    }

    /// [`Self::get`] for bodies up to `max_body` bytes (a DAT, a loader).
    pub async fn get_large(
        &self,
        limiter: &str,
        url: &str,
        validators: &Validators,
        max_body: u64,
    ) -> Result<Fetched, NetError> {
        let mut headers = HeaderMap::new();
        if let Some(etag) = &validators.etag
            && let Ok(value) = HeaderValue::from_str(etag)
        {
            headers.insert(header::IF_NONE_MATCH, value);
        }
        if let Some(date) = &validators.last_modified
            && let Ok(value) = HeaderValue::from_str(date)
        {
            headers.insert(header::IF_MODIFIED_SINCE, value);
        }
        let Some(response) = self.send(limiter, url, headers, None).await? else {
            return Ok(Fetched::NotFound);
        };
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(Fetched::NotModified);
        }
        read_body(response, max_body).await.map(Fetched::Ok)
    }

    /// GETs part of `url` (ADR-0035): `None` for 404 or 410. A server that
    /// answers a range request with the whole file is accepted only while
    /// the file fits in `max_body`, and the range is cut out of it.
    pub async fn get_range(
        &self,
        limiter: &str,
        url: &str,
        range: ByteRange,
        max_body: u64,
        timeout: Option<Duration>,
    ) -> Result<Option<Ranged>, NetError> {
        let value = match range {
            ByteRange::Span { start, len } => {
                format!("bytes={start}-{}", start + len.max(1) - 1)
            }
            ByteRange::Suffix(len) => format!("bytes=-{}", len.max(1)),
        };
        let mut headers = HeaderMap::new();
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.insert(header::RANGE, value);
        }
        let Some(response) = self.send(limiter, url, headers, timeout).await? else {
            return Ok(None);
        };
        let status = response.status();
        let content_range = header_string(response.headers(), header::CONTENT_RANGE);
        let body = read_body(response, max_body).await?;
        if status == StatusCode::PARTIAL_CONTENT {
            let (start, total) = content_range
                .as_deref()
                .and_then(parse_content_range)
                .ok_or_else(|| {
                    NetError::Http(format!(
                        "{url}: a partial answer without a usable Content-Range"
                    ))
                })?;
            return Ok(Some(Ranged {
                bytes: body.bytes,
                start,
                total,
                etag: body.etag,
            }));
        }
        // 200: the whole file.
        let total = body.bytes.len() as u64;
        let (start, len) = match range {
            ByteRange::Span { start, len } => (start.min(total), len),
            ByteRange::Suffix(len) => (total.saturating_sub(len), len),
        };
        let end = start.saturating_add(len).min(total);
        let bytes = body.bytes[start as usize..end as usize].to_vec();
        Ok(Some(Ranged {
            bytes,
            start,
            total,
            etag: body.etag,
        }))
    }

    /// Sends a GET, waiting for the limiter of `limiter` before each try
    /// and retrying on 429 and 5xx with back-off. `None` for 404 and 410;
    /// any other status that is neither a success nor 304 is an error.
    async fn send(
        &self,
        limiter: &str,
        url: &str,
        headers: HeaderMap,
        timeout: Option<Duration>,
    ) -> Result<Option<reqwest::Response>, NetError> {
        if offline() && !is_loopback(url) {
            return Err(NetError::Http(format!(
                "{url}: network access is off (YAOPLM_OFFLINE)"
            )));
        }
        let mut attempt = 0u32;
        loop {
            self.throttle(limiter).await;
            let mut request = self.client.get(url).headers(headers.clone());
            if let Some(timeout) = timeout {
                request = request.timeout(timeout);
            }
            let retry_after = match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    match status {
                        StatusCode::NOT_FOUND | StatusCode::GONE => return Ok(None),
                        s if s.is_success() || s == StatusCode::NOT_MODIFIED => {
                            return Ok(Some(response));
                        }
                        s if s == StatusCode::TOO_MANY_REQUESTS || s.is_server_error() => {
                            retry_after_header(response.headers())
                        }
                        s => {
                            return Err(NetError::Status {
                                url: url.to_owned(),
                                status: s.as_u16(),
                            });
                        }
                    }
                }
                Err(err)
                    if attempt < RETRIES
                        && (err.is_connect() || err.is_timeout() || err.is_request()) =>
                {
                    None
                }
                Err(err) => return Err(NetError::Http(format!("{url}: {err}"))),
            };
            if attempt >= RETRIES {
                return Err(NetError::Http(format!(
                    "{url}: gave up after {RETRIES} retries"
                )));
            }
            let backoff = Duration::from_secs(1 << attempt);
            tokio::time::sleep(retry_after.unwrap_or(backoff).min(MAX_BACKOFF)).await;
            attempt += 1;
        }
    }
}

/// `YAOPLM_OFFLINE` is set to anything but `0` or nothing.
fn offline() -> bool {
    std::env::var("YAOPLM_OFFLINE").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// A URL of this machine: the local servers of tests and the e2e run.
fn is_loopback(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    let host = url.host_str().unwrap_or("");
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// `bytes 100-199/5539563942`: the first byte and the size of the file.
fn parse_content_range(value: &str) -> Option<(u64, u64)> {
    let rest = value.trim().strip_prefix("bytes")?.trim_start();
    let (span, total) = rest.split_once('/')?;
    let (start, end) = span.split_once('-')?;
    let (start, end, total) = (
        start.trim().parse::<u64>().ok()?,
        end.trim().parse::<u64>().ok()?,
        total.trim().parse::<u64>().ok()?,
    );
    (start <= end && end < total).then_some((start, total))
}

fn retry_after_header(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

fn header_string(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// `attachment; filename="Sony - PlayStation 2 - Datfile (11845).zip"`.
fn disposition_file_name(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::CONTENT_DISPOSITION)?.to_str().ok()?;
    let start = value.find("filename=")? + "filename=".len();
    let rest = value[start..].trim();
    let name = match rest.strip_prefix('"') {
        Some(quoted) => &quoted[..quoted.find('"')?],
        None => rest.split(';').next()?.trim(),
    };
    let name = name.rsplit(['/', '\\']).next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

async fn read_body(mut response: reqwest::Response, max_body: u64) -> Result<Body, NetError> {
    let url = response.url().to_string();
    if response.content_length().is_some_and(|len| len > max_body) {
        return Err(NetError::TooLarge(url));
    }
    let headers = response.headers().clone();
    // Chunk by chunk: without a Content-Length the size shows only as the
    // body arrives, and it must not all be held first.
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| NetError::Http(format!("{url}: {e}")))?
    {
        if (bytes.len() + chunk.len()) as u64 > max_body {
            return Err(NetError::TooLarge(url));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(Body {
        bytes,
        content_type: header_string(&headers, header::CONTENT_TYPE),
        etag: header_string(&headers, header::ETAG),
        last_modified: header_string(&headers, header::LAST_MODIFIED),
        file_name: disposition_file_name(&headers),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_from_content_disposition() {
        let name = |value: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(value).unwrap(),
            );
            disposition_file_name(&headers)
        };
        assert_eq!(
            name(r#"attachment; filename="Sony - PlayStation 2 - Datfile (11845) (2026-10-23 01-56-22).zip""#)
                .as_deref(),
            Some("Sony - PlayStation 2 - Datfile (11845) (2026-10-23 01-56-22).zip")
        );
        assert_eq!(
            name("attachment; filename=a.zip; size=3").as_deref(),
            Some("a.zip")
        );
        assert_eq!(
            name(r#"attachment; filename="../../x.zip""#).as_deref(),
            Some("x.zip")
        );
        assert_eq!(name("inline"), None);
    }

    #[test]
    fn loopback_hosts() {
        assert!(is_loopback("http://127.0.0.1:7879/x"));
        assert!(is_loopback("http://localhost/x"));
        assert!(is_loopback("http://[::1]:80/x"));
        assert!(!is_loopback("https://archive.org/download/x"));
        assert!(!is_loopback("not a url"));
    }

    #[test]
    fn content_ranges() {
        assert_eq!(
            parse_content_range("bytes 100-199/5539563942"),
            Some((100, 5_539_563_942))
        );
        assert_eq!(parse_content_range("bytes 0-0/1"), Some((0, 1)));
        assert_eq!(parse_content_range("bytes 0-9/*"), None);
        assert_eq!(parse_content_range("bytes 9-0/10"), None);
        assert_eq!(parse_content_range("bytes 0-10/10"), None);
        assert_eq!(parse_content_range("items 0-1/2"), None);
    }
}
