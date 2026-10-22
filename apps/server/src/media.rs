//! `GET /media/art?path=<host path>`: a cover from a library root, for
//! `<img>` tags in the web UI (ART-2).
//!
//! The same allow-list as `/api` applies (ADR-0009): the file must sit in
//! an `ART` folder inside one of the server's roots, be named like OPL art
//! (`<ID>_<SUFFIX>.png|jpg|bmp`) and be a regular file. Nothing else on the
//! host is served. Responses carry an `ETag` from size and mtime, so a
//! cover the UI already has costs one 304.

use std::path::Path;
use std::time::UNIX_EPOCH;

use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use yaoplm_api::{ApiError, PathPolicy};
use yaoplm_formats::art::parse_art_file_name;

use crate::AppState;
use crate::rpc::api_error;

/// `path` is the absolute host path of the file, as `ScanLibrary` reported
/// it. The UI adds `v=<mtime>` as a cache buster; it is ignored here.
#[derive(Debug, Deserialize)]
pub struct ArtQuery {
    pub path: String,
}

fn content_type(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "bmp" => Some("image/bmp"),
        _ => None,
    }
}

/// Checks the path against the policy and OPL's art naming.
fn resolve(policy: &PathPolicy, raw: &str) -> Result<(std::path::PathBuf, &'static str), ApiError> {
    let path = policy.resolve(raw)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ApiError::bad_request("not a file name"))?;
    let parsed = parse_art_file_name(name)
        .ok_or_else(|| ApiError::bad_request(format!("{name} is not an art file name")))?;
    let mime = content_type(&parsed.extension)
        .ok_or_else(|| ApiError::bad_request(format!("{name} is not an image OPL reads")))?;
    let in_art_folder = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case("ART"));
    if !in_art_folder {
        return Err(ApiError::forbidden(
            "only files in an ART folder are served",
        ));
    }
    Ok((path, mime))
}

pub async fn art(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<ArtQuery>,
) -> Response {
    let policy = PathPolicy::Within(state.config().roots.clone());
    let (path, mime) = match resolve(&policy, &query.path) {
        Ok(found) => found,
        Err(error) => return api_error(error),
    };
    let meta = match tokio::fs::metadata(&path).await {
        Ok(meta) if meta.is_file() => meta,
        _ => return api_error(ApiError::not_found("no such cover")),
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis());
    let etag = format!("\"{:x}-{:x}\"", meta.len(), mtime);
    if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|sent| sent.split(',').any(|t| t.trim() == etag))
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) => return api_error(ApiError::internal(err.to_string())),
    };
    (
        [
            (header::CONTENT_TYPE, mime.to_owned()),
            (header::CACHE_CONTROL, "private, max-age=3600".to_owned()),
            (header::ETAG, etag),
        ],
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_art_files_inside_the_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let art = root.join("ART");
        std::fs::create_dir_all(&art).unwrap();
        std::fs::write(art.join("SLUS_200.62_COV.png"), b"png").unwrap();
        std::fs::write(art.join("notes.txt"), b"x").unwrap();
        std::fs::write(root.join("SLUS_200.62_COV.png"), b"png").unwrap();
        let policy = PathPolicy::Within(vec![root.clone()]);
        let ok = resolve(&policy, &art.join("SLUS_200.62_COV.png").to_string_lossy()).unwrap();
        assert_eq!(ok.1, "image/png");
        assert!(resolve(&policy, &art.join("notes.txt").to_string_lossy()).is_err());
        assert!(resolve(&policy, &root.join("SLUS_200.62_COV.png").to_string_lossy()).is_err());
        assert!(
            resolve(
                &PathPolicy::Within(Vec::new()),
                &art.join("SLUS_200.62_COV.png").to_string_lossy()
            )
            .is_err()
        );
        assert!(resolve(&policy, "../ART/SLUS_200.62_COV.png").is_err());
    }
}
