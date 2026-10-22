//! YAOPLM desktop app: a Tauri 2 window around the shared SPA. The commands
//! are a thin adapter over `yaoplm_api::Api` (ARCHITECTURE.md §4).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::ipc::Channel;
use tauri::{State, Webview};
use yaoplm_api::{Api, ApiError, JobEvent, Request, Response, Subscription};

/// One event stream per webview, keyed by its label.
#[derive(Default)]
struct Subscriptions(Mutex<HashMap<String, Subscription>>);

#[tauri::command]
async fn rpc(api: State<'_, Arc<Api>>, request: Request) -> Result<Response, ApiError> {
    let api = Arc::clone(&api);
    tauri::async_runtime::spawn_blocking(move || api.dispatch(request))
        .await
        .map_err(|err| ApiError::internal(err.to_string()))?
}

/// Streams job events to the calling webview. Subscribing again, e.g. after a
/// page reload, replaces the previous stream.
#[tauri::command]
fn subscribe(
    webview: Webview,
    api: State<'_, Arc<Api>>,
    subscriptions: State<'_, Subscriptions>,
    on_event: Channel<JobEvent>,
) {
    let subscription = api.subscribe(move |event| {
        // Fails only when the page is gone; the next subscribe replaces us.
        let _ = on_event.send(event.clone());
    });
    subscriptions
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(webview.label().to_owned(), subscription);
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(Arc::new(Api::new()))
        .manage(Subscriptions::default())
        .invoke_handler(tauri::generate_handler![rpc, subscribe])
        .run(tauri::generate_context!())
}
