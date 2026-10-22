use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::{EmbeddedFile, RustEmbed};

/// The built SPA. Release builds embed it; debug builds read it from disk, so
/// `pnpm -C web build` is picked up without recompiling the server.
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Web;

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !path.is_empty()
        && let Some(file) = Web::get(path)
    {
        return file_response(path, file);
    }

    // Client-side routes (no file extension) get the SPA shell.
    let last_segment = path.rsplit('/').next().unwrap_or_default();
    if last_segment.contains('.') {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Web::get("index.html") {
        Some(index) => file_response("index.html", index),
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            "The web UI is not built. Run `pnpm -C web build`, then restart the server.",
        )
            .into_response(),
    }
}

fn file_response(path: &str, file: EmbeddedFile) -> Response {
    // Vite fingerprints everything under assets/, so it never changes.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
            (header::CACHE_CONTROL, cache.to_owned()),
        ],
        file.data,
    )
        .into_response()
}
