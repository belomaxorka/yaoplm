use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use yaoplm_api::{ApiError, ErrorCode};

use crate::AppState;

pub async fn handle(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    // Cross-site forms can only send text/plain or form bodies; insisting on
    // JSON forces a CORS preflight that we never answer.
    if !is_json(&headers) {
        return error_response(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            ApiError::bad_request("expected Content-Type: application/json"),
        );
    }
    let Ok(body) = String::from_utf8(body.to_vec()) else {
        return api_error(ApiError::bad_request("request body is not UTF-8"));
    };

    let api = state.api().clone();
    match tokio::task::spawn_blocking(move || api.dispatch_json(&body)).await {
        Ok(Ok(response)) => Json(response).into_response(),
        Ok(Err(error)) => api_error(error),
        Err(join) => api_error(ApiError::internal(join.to_string())),
    }
}

pub fn api_error(error: ApiError) -> Response {
    let status = match error.code {
        ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
        ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Cancelled => StatusCode::CONFLICT,
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    };
    error_response(status, error)
}

pub fn error_response(status: StatusCode, error: ApiError) -> Response {
    (status, Json(error)).into_response()
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}
