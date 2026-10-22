use std::net::IpAddr;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;
use yaoplm_api::ApiError;

use crate::AppState;
use crate::rpc::error_response;

/// Guards `/api` (ADR-0009, ADR-0012): same-origin requests only, the token
/// when one is configured, and without a token a loopback `Host` (DNS
/// rebinding cannot forge a token, so a reverse proxy may pass its own host).
pub async fn require(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let config = state.config();
    let headers = request.headers();

    if config.loopback_only && config.token.is_none() && !host_is_loopback(headers) {
        return forbidden("Host header is not a loopback address");
    }
    if !origin_matches_host(headers) {
        return forbidden("cross-origin requests are not allowed");
    }

    if let Some(expected) = &config.token {
        // Browsers cannot set headers on WebSocket handshakes, so the events
        // stream also accepts `?token=`.
        let provided = bearer_token(headers).or_else(|| {
            (request.uri().path() == "/api/events")
                .then(|| query_token(request.uri().query()))
                .flatten()
        });
        if !provided.is_some_and(|token| constant_time_eq(token.as_bytes(), expected.as_bytes())) {
            return error_response(
                StatusCode::UNAUTHORIZED,
                ApiError::unauthorized("missing or invalid token"),
            );
        }
    }

    next.run(request).await
}

fn forbidden(message: &str) -> Response {
    error_response(
        StatusCode::FORBIDDEN,
        ApiError::unauthorized(message.to_owned()),
    )
}

fn header_str(headers: &HeaderMap, name: header::HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn host_is_loopback(headers: &HeaderMap) -> bool {
    let Some(host) = header_str(headers, header::HOST) else {
        return false;
    };
    let name = strip_port(host);
    let name = name
        .strip_prefix('[')
        .and_then(|n| n.strip_suffix(']'))
        .unwrap_or(name);
    name.eq_ignore_ascii_case("localhost")
        || name.to_ascii_lowercase().ends_with(".localhost")
        || name.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn strip_port(host: &str) -> &str {
    match host.rfind(':') {
        // `[::1]:7878` or `127.0.0.1:7878`; a bare `::1` has no brackets.
        Some(i) if host.starts_with('[') || host[..i].find(':').is_none() => &host[..i],
        _ => host,
    }
}

fn origin_matches_host(headers: &HeaderMap) -> bool {
    let Some(origin) = header_str(headers, header::ORIGIN) else {
        // Non-browser clients (CLI, curl) send no Origin.
        return true;
    };
    let Some(host) = header_str(headers, header::HOST) else {
        return false;
    };
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|authority| authority.eq_ignore_ascii_case(host))
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    header_str(headers, header::AUTHORIZATION)?.strip_prefix("Bearer ")
}

fn query_token(query: Option<&str>) -> Option<&str> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(header::HeaderName, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn loopback_hosts() {
        for host in [
            "localhost",
            "localhost:7878",
            "app.localhost:5173",
            "127.0.0.1:7878",
            "127.0.0.2",
            "[::1]:7878",
            "::1",
        ] {
            assert!(
                host_is_loopback(&headers(&[(header::HOST, host)])),
                "{host}"
            );
        }
        for host in [
            "evil.example:7878",
            "192.168.1.10:7878",
            "localhost.evil.example",
            "[::2]:7878",
        ] {
            assert!(
                !host_is_loopback(&headers(&[(header::HOST, host)])),
                "{host}"
            );
        }
    }

    #[test]
    fn origin_must_match_host() {
        let same = headers(&[
            (header::HOST, "127.0.0.1:7878"),
            (header::ORIGIN, "http://127.0.0.1:7878"),
        ]);
        let cross = headers(&[
            (header::HOST, "127.0.0.1:7878"),
            (header::ORIGIN, "http://evil.example"),
        ]);
        let null = headers(&[(header::HOST, "127.0.0.1:7878"), (header::ORIGIN, "null")]);
        assert!(origin_matches_host(&same));
        assert!(!origin_matches_host(&cross));
        assert!(!origin_matches_host(&null));
        assert!(origin_matches_host(&headers(&[(header::HOST, "x")])));
    }

    #[test]
    fn token_comparison() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret2"));
        assert_eq!(query_token(Some("a=1&token=abc")), Some("abc"));
        assert_eq!(query_token(Some("a=1")), None);
    }
}
