#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tower::ServiceExt;
use yaoplm_api::{Api, JobEvent, JobId};
use yaoplm_server::{AppState, Config};

const HOST: &str = "127.0.0.1:7878";

fn state(token: Option<&str>) -> AppState {
    AppState::new(
        Arc::new(Api::new()),
        Config {
            token: token.map(str::to_owned),
            roots: Vec::new(),
            loopback_only: true,
        },
    )
}

fn rpc_post() -> axum::http::request::Builder {
    Request::post("/api/rpc")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
}

async fn send(state: AppState, request: Request<Body>) -> (StatusCode, Value) {
    let response = yaoplm_server::app(state).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn health_reports_version() {
    let request = Request::get("/health").body(Body::empty()).unwrap();
    let (status, body) = send(state(None), request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!({"status": "ok", "version": env!("CARGO_PKG_VERSION")})
    );
}

#[tokio::test]
async fn rpc_ping() {
    let body = r#"{"type":"Ping"}"#;
    let request = rpc_post().body(Body::from(body)).unwrap();
    let (status, body) = send(state(None), request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, json!({"type": "Pong"}));
}

#[tokio::test]
async fn rpc_malformed_is_bad_request() {
    let request = rpc_post().body(Body::from("{")).unwrap();
    let (status, body) = send(state(None), request).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "BadRequest");
}

#[tokio::test]
async fn rpc_requires_json_content_type() {
    let request = Request::post("/api/rpc")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from(r#"{"type":"Ping"}"#))
        .unwrap();
    let (status, _) = send(state(None), request).await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn rpc_rejects_cross_origin_and_foreign_host() {
    let body = r#"{"type":"Ping"}"#;
    let cross_origin = rpc_post()
        .header(header::ORIGIN, "http://evil.example")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        send(state(None), cross_origin).await.0,
        StatusCode::FORBIDDEN
    );

    let rebinding = Request::post("/api/rpc")
        .header(header::HOST, "evil.example:7878")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(send(state(None), rebinding).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn token_is_enforced_when_configured() {
    let body = r#"{"type":"Ping"}"#;

    let missing = rpc_post().body(Body::from(body)).unwrap();
    let (status, error) = send(state(Some("s3cret")), missing).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["code"], "Unauthorized");

    let wrong = rpc_post()
        .header(header::AUTHORIZATION, "Bearer nope")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        send(state(Some("s3cret")), wrong).await.0,
        StatusCode::UNAUTHORIZED
    );

    let right = rpc_post()
        .header(header::AUTHORIZATION, "Bearer s3cret")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(send(state(Some("s3cret")), right).await.0, StatusCode::OK);

    // The query-string token is only for the WebSocket handshake.
    let query = Request::post("/api/rpc?token=s3cret")
        .header(header::HOST, HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(
        send(state(Some("s3cret")), query).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn token_allows_reverse_proxy_host() {
    let body = r#"{"type":"Ping"}"#;
    let request = Request::post("/api/rpc")
        .header(header::HOST, "ps2.home.example")
        .header(header::ORIGIN, "https://ps2.home.example")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::AUTHORIZATION, "Bearer s3cret")
        .body(Body::from(body))
        .unwrap();
    assert_eq!(send(state(Some("s3cret")), request).await.0, StatusCode::OK);
}

#[tokio::test]
async fn unknown_api_route_is_not_the_spa() {
    let request = Request::get("/api/nope")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(state(None), request).await.0, StatusCode::NOT_FOUND);
}

/// Game pages carry file names in the path (`/library/game/img:...iso`);
/// a dot must not turn a client route into a 404 (an M3 bug).
#[tokio::test]
async fn deep_links_with_dots_get_the_spa() {
    let request = Request::get("/library/game/img%3AE%3A%5CCD%5CSLUS_200.62.Game.iso")
        .body(Body::empty())
        .unwrap();
    let (status, _) = send(state(None), request).await;
    // web/dist may be missing on a bare checkout: then the shell is 503, never 404.
    assert_ne!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_static_file_is_404() {
    let request = Request::get("/assets/missing-file.js")
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(state(None), request).await.0, StatusCode::NOT_FOUND);
}

/// Opens a real WebSocket to /api/events and checks that published job events
/// arrive as JSON text frames.
#[tokio::test]
async fn events_stream_job_events() {
    let state = state(None);
    let api = Arc::clone(state.api());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, yaoplm_server::app(state))
            .await
            .unwrap();
    });

    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    let handshake = format!(
        "GET /api/events HTTP/1.1\r\nHost: {addr}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    stream.write_all(handshake.as_bytes()).await.unwrap();

    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await.unwrap();
    let head = String::from_utf8_lossy(&buf[..n]);
    assert!(head.starts_with("HTTP/1.1 101"), "handshake failed: {head}");

    api.events().publish(&JobEvent::Started { job: JobId(42) });

    // Unmasked server text frame: 0x81, payload length, payload.
    let mut frame = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut frame))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frame[0], 0x81);
    let mut payload = vec![0u8; usize::from(frame[1] & 0x7f)];
    stream.read_exact(&mut payload).await.unwrap();
    let event: Value = serde_json::from_slice(&payload).unwrap();
    assert_eq!(event, json!({"type": "Started", "job": 42}));
}

#[tokio::test]
async fn paths_outside_roots_are_forbidden() {
    let dir = std::env::temp_dir();
    let state = AppState::new(
        Arc::new(Api::with_roots(Vec::new())),
        Config {
            token: None,
            roots: Vec::new(),
            loopback_only: true,
        },
    );
    let body = json!({"type": "ScanLibrary", "root": dir}).to_string();
    let request = rpc_post().body(Body::from(body)).unwrap();
    let (status, body) = send(state, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "Forbidden");
}
