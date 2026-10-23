//! `yaoplm-server`: the web UI and the RPC API over HTTP (ARCHITECTURE.md §9).
//!
//! Routes:
//! - `GET /health`: liveness, no auth;
//! - `POST /api/rpc`: one `yaoplm_api::Request` in, one `Response` or `ApiError` out;
//! - `GET /api/events`: WebSocket stream of `JobEvent`s as JSON text frames;
//! - `GET /media/art?path=`: a cover from a library root (ART-2);
//! - everything else: the embedded SPA from `web/dist`.

mod assets;
mod auth;
mod events;
mod media;
mod rpc;

use std::path::PathBuf;
use std::sync::Arc;

use axum::routing::{get, post};
use axum::{Json, Router, middleware};
use tokio::sync::broadcast;
use yaoplm_api::{Api, JobEvent, Subscription};

#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Required on every `/api` request when set.
    pub token: Option<String>,
    /// Allow-list of library roots the UI may open.
    pub roots: Vec<PathBuf>,
    /// The server listens on loopback only. Without a token, `/api` then
    /// also rejects foreign `Host` headers (DNS rebinding).
    pub loopback_only: bool,
}

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

struct Inner {
    api: Arc<Api>,
    config: Config,
    events: broadcast::Sender<JobEvent>,
    _subscription: Subscription,
}

impl AppState {
    pub fn new(api: Arc<Api>, config: Config) -> Self {
        let (events, _) = broadcast::channel(256);
        let forward = events.clone();
        // No receivers just means no browser is connected; dropping is fine.
        let subscription = api.subscribe(move |event| {
            let _ = forward.send(event.clone());
        });
        Self(Arc::new(Inner {
            api,
            config,
            events,
            _subscription: subscription,
        }))
    }

    pub fn api(&self) -> &Arc<Api> {
        &self.0.api
    }

    pub fn config(&self) -> &Config {
        &self.0.config
    }
}

pub fn app(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/rpc", post(rpc::handle))
        .route("/api/events", get(events::handle))
        .route("/media/art", get(media::art))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require));

    Router::new()
        .route("/health", get(health))
        .merge(api)
        .fallback(assets::serve)
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "version": yaoplm_api::VERSION }))
}
