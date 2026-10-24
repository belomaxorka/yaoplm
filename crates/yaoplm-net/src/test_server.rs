//! The smallest HTTP/1.1 server that satisfies reqwest, for tests: one
//! request per connection, canned answers routed by path, conditional
//! requests, range requests and servers that fail a few times first.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A canned answer.
#[derive(Clone, Default)]
pub(crate) struct Reply {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    /// Answer 500 this many times before the real answer.
    pub failures: u32,
}

impl Reply {
    pub fn ok(content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type", content_type.to_owned())],
            body,
            failures: 0,
        }
    }
}

pub(crate) struct Seen {
    /// The path with its query, and the headers in lower case.
    pub requests: Vec<(String, HashMap<String, String>)>,
    pub times: Vec<Instant>,
}

impl Seen {
    /// How many requests asked for `path` (without its query).
    pub fn count(&self, path: &str) -> usize {
        self.requests
            .iter()
            .filter(|(p, _)| p.split('?').next() == Some(path))
            .count()
    }
}

/// `bytes=10-19` or `bytes=-5` against a body of `len` bytes.
fn range_of(header: &str, len: usize) -> Option<(usize, usize)> {
    let spec = header.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let (start, end) = if start.is_empty() {
        let n: usize = end.parse().ok()?;
        (len.saturating_sub(n), len.checked_sub(1)?)
    } else {
        let start: usize = start.parse().ok()?;
        let end = if end.is_empty() {
            len.checked_sub(1)?
        } else {
            end.parse::<usize>().ok()?.min(len.checked_sub(1)?)
        };
        (start, end)
    };
    (start <= end && end < len).then_some((start, end))
}

/// Routes that a test may add to once it knows the server's address.
pub(crate) type Routes = Arc<Mutex<HashMap<String, Reply>>>;

/// Serves `routes` on a free local port: the base URL and what was asked.
pub(crate) async fn serve(routes: HashMap<String, Reply>) -> (String, Arc<Mutex<Seen>>) {
    let (base, seen, _) = serve_routes(routes).await;
    (base, seen)
}

/// [`serve`], with the routes kept open for changes.
pub(crate) async fn serve_routes(
    routes: HashMap<String, Reply>,
) -> (String, Arc<Mutex<Seen>>, Routes) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Seen {
        requests: Vec::new(),
        times: Vec::new(),
    }));
    let routes = Arc::new(Mutex::new(routes));
    let handle = Arc::clone(&routes);
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let routes = Arc::clone(&routes);
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
                let reply = {
                    let mut routes = routes.lock().unwrap();
                    // By the whole path, else by the path without its query.
                    let key = if routes.contains_key(&path) {
                        path.clone()
                    } else {
                        path.split('?').next().unwrap_or("/").to_owned()
                    };
                    routes
                        .get_mut(&key)
                        .map(|reply| {
                            if reply.failures > 0 {
                                reply.failures -= 1;
                                Reply {
                                    status: 500,
                                    ..Reply::default()
                                }
                            } else {
                                reply.clone()
                            }
                        })
                        .unwrap_or(Reply {
                            status: 404,
                            body: b"nope".to_vec(),
                            ..Reply::default()
                        })
                };
                // A conditional request for the current ETag gets 304.
                let etag = reply
                    .headers
                    .iter()
                    .find(|(k, _)| *k == "ETag")
                    .map(|(_, v)| v.clone());
                let mut extra = Vec::new();
                let (status, body) = match (etag, headers.get("if-none-match")) {
                    (Some(etag), Some(sent)) if *sent == etag && reply.status == 200 => {
                        (304, Vec::new())
                    }
                    _ => match headers.get("range") {
                        Some(range) if reply.status == 200 => {
                            match range_of(range, reply.body.len()) {
                                Some((start, end)) => {
                                    extra.push(format!(
                                        "Content-Range: bytes {start}-{end}/{}",
                                        reply.body.len()
                                    ));
                                    (206, reply.body[start..=end].to_vec())
                                }
                                None => (416, Vec::new()),
                            }
                        }
                        _ => (reply.status, reply.body.clone()),
                    },
                };
                let mut out = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in &reply.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                for line in extra {
                    out.push_str(&line);
                    out.push_str("\r\n");
                }
                out.push_str("\r\n");
                let _ = socket.write_all(out.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    (base, seen, handle)
}

#[test]
fn ranges() {
    assert_eq!(range_of("bytes=0-3", 10), Some((0, 3)));
    assert_eq!(range_of("bytes=-4", 10), Some((6, 9)));
    assert_eq!(range_of("bytes=-40", 10), Some((0, 9)));
    assert_eq!(range_of("bytes=8-20", 10), Some((8, 9)));
    assert_eq!(range_of("bytes=12-20", 10), None);
}
