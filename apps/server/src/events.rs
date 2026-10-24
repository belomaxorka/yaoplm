use std::time::Duration;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use tokio::sync::broadcast::{self, error::RecvError};
use yaoplm_api::JobEvent;

use crate::AppState;

/// A ping this often keeps an idle stream open through proxies that close
/// quiet connections (nginx: 60 s by default); the page treats a closed
/// stream as a link that may be lost.
const KEEPALIVE: Duration = Duration::from_secs(25);

pub async fn handle(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    // Subscribe before the handshake completes so no event published after
    // the client sees the connection open is lost.
    let receiver = state.0.events.subscribe();
    upgrade.on_upgrade(move |socket| forward(socket, receiver))
}

async fn forward(mut socket: WebSocket, mut events: broadcast::Receiver<JobEvent>) {
    let mut keepalive = tokio::time::interval(KEEPALIVE);
    keepalive.tick().await;
    loop {
        tokio::select! {
            _ = keepalive.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            event = events.recv() => match event {
                Ok(event) => {
                    let Ok(text) = serde_json::to_string(&event) else { continue };
                    if socket.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "events client is lagging; events dropped");
                }
                Err(RecvError::Closed) => break,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {}
            },
        }
    }
}
