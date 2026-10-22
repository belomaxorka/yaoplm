//! The one HTTP client every source shares (ADR-0007, research/03 §7.2).
//!
//! - User-Agent `YAOPLM/<version> (+https://github.com/belomaxorka/yaoplm)`;
//! - at most one request per second per source (a token bucket of one);
//! - conditional requests with `If-None-Match` / `If-Modified-Since`;
//! - exponential back-off on 429 and 5xx, honouring `Retry-After`;
//! - a size cap, so a wrong URL cannot pull gigabytes into memory.

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fetched {
    Ok(Body),
    /// 304: what the cache has is still current.
    NotModified,
    /// 404 or 410.
    NotFound,
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

        let mut attempt = 0u32;
        loop {
            self.throttle(limiter).await;
            let outcome = self.client.get(url).headers(headers.clone()).send().await;
            let retry_after = match outcome {
                Ok(response) => {
                    let status = response.status();
                    match status {
                        StatusCode::NOT_MODIFIED => return Ok(Fetched::NotModified),
                        StatusCode::NOT_FOUND | StatusCode::GONE => return Ok(Fetched::NotFound),
                        s if s.is_success() => return read_body(response).await,
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

async fn read_body(response: reqwest::Response) -> Result<Fetched, NetError> {
    let url = response.url().to_string();
    if response
        .content_length()
        .is_some_and(|len| len > MAX_BODY as u64)
    {
        return Err(NetError::TooLarge(url));
    }
    let headers = response.headers().clone();
    let bytes = response
        .bytes()
        .await
        .map_err(|e| NetError::Http(format!("{url}: {e}")))?;
    if bytes.len() > MAX_BODY {
        return Err(NetError::TooLarge(url));
    }
    Ok(Fetched::Ok(Body {
        bytes: bytes.to_vec(),
        content_type: header_string(&headers, header::CONTENT_TYPE),
        etag: header_string(&headers, header::ETAG),
        last_modified: header_string(&headers, header::LAST_MODIFIED),
    }))
}
