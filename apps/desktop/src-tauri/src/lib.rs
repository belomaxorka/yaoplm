//! YAOPLM desktop app: a Tauri 2 window around the shared SPA. The commands
//! are a thin adapter over `yaoplm_api::Api` (ARCHITECTURE.md §4).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};
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

/// Lets the page load `ART/*` of the open library root through the asset
/// protocol (`convertFileSrc`), and nothing else on the disk (ART-2). The
/// folder may not exist yet; it is allowed again after every scan.
#[tauri::command]
fn allow_art(app: AppHandle, root: String) -> Result<bool, ApiError> {
    let art = Path::new(&root).join("ART");
    if !art.is_dir() {
        return Ok(false);
    }
    app.asset_protocol_scope()
        .allow_directory(&art, false)
        .map(|()| true)
        .map_err(|err| ApiError::internal(err.to_string()))
}

pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        // Native file and folder pickers for the Add games screen (IMP-1).
        .plugin(tauri_plugin_dialog::init())
        // Pasting a cover from the clipboard (ART-3).
        .plugin(tauri_plugin_clipboard_manager::init())
        // Links to web pages (sources, licences) open in the system browser.
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::new(Api::new()))
        .manage(Subscriptions::default())
        .invoke_handler(tauri::generate_handler![rpc, subscribe, allow_art])
        .run(tauri::generate_context!())
}
