use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::phrase::Phrase;

/// Error returned to front ends. Transports map `code` to their own status
/// (HTTP status, CLI exit code). `message` is English, for logs and the
/// CLI; the UI shows `phrase` in its own language when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[error("{message} ({code:?})")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<Phrase>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum ErrorCode {
    /// The request is malformed or names an unknown operation.
    BadRequest,
    /// Server mode only: missing or wrong token.
    Unauthorized,
    /// Server mode only: the path is outside the allowed library roots.
    Forbidden,
    NotFound,
    Cancelled,
    Internal,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            phrase: None,
        }
    }

    /// An error the UI can translate; `message` is the phrase in English.
    pub fn phrased(code: ErrorCode, phrase: &yaoplm_core::Phrase) -> Self {
        Self {
            code,
            message: phrase.english(),
            phrase: Some(Phrase::from(phrase)),
        }
    }

    /// `Cancelled`, as every job reports it.
    pub fn cancelled() -> Self {
        Self::phrased(
            ErrorCode::Cancelled,
            &yaoplm_core::Phrase::error("cancelled"),
        )
    }

    /// An internal error with the underlying message.
    pub fn internal_error(error: impl ToString) -> Self {
        Self::phrased(
            ErrorCode::Internal,
            &yaoplm_core::Phrase::error("internal").with("error", error.to_string()),
        )
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

/// A failed request or a file that is not a Redump DAT, with `code` for
/// the transport.
pub(crate) fn net_error(code: ErrorCode, err: &yaoplm_net::NetError) -> ApiError {
    use yaoplm_core::Phrase;
    use yaoplm_net::NetError;
    let phrase = match err {
        NetError::Http(error) => Phrase::error("network").with("error", error),
        NetError::Zip(error) => Phrase::error("network").with("error", format!("zip: {error}")),
        NetError::Status { url, status } => Phrase::error("httpStatus")
            .with("url", url)
            .with("status", u64::from(*status)),
        NetError::TooLarge(url) => Phrase::error("responseTooLarge").with("url", url),
        NetError::Image(error) => Phrase::error("imageError").with("error", error),
        NetError::Cache(error) => Phrase::error("cacheError").with("error", error),
        NetError::Dat(error) => Phrase::error("datInvalid").with("error", error),
        NetError::Io { path, source } => Phrase::error("pathError")
            .with("path", path.display().to_string())
            .with("error", source.to_string()),
    };
    ApiError::phrased(code, &phrase)
}
