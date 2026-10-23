//! Which paths a front end may touch (ADR-0009, ADR-0012).

use std::path::{Path, PathBuf};

use yaoplm_core::Phrase;

use crate::{ApiError, ErrorCode};

/// The desktop app and the CLI may open any path the user picks; the server
/// only paths inside its `--root` allow-list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum PathPolicy {
    #[default]
    Any,
    /// Canonical roots. Empty means nothing may be opened.
    Within(Vec<PathBuf>),
}

impl PathPolicy {
    /// Resolves `raw` (symlinks and `..` included) and checks it.
    pub fn resolve(&self, raw: &str) -> Result<PathBuf, ApiError> {
        if raw.is_empty() {
            return Err(ApiError::phrased(
                ErrorCode::BadRequest,
                &Phrase::error("emptyPath"),
            ));
        }
        let path = std::fs::canonicalize(raw).map_err(|err| {
            ApiError::phrased(
                ErrorCode::NotFound,
                &Phrase::error("pathError")
                    .with("path", raw)
                    .with("error", err.to_string()),
            )
        })?;
        match self {
            Self::Any => Ok(path),
            Self::Within(roots) if roots.iter().any(|root| path.starts_with(root)) => Ok(path),
            Self::Within(_) => Err(ApiError::phrased(
                ErrorCode::Forbidden,
                &Phrase::error("outsideRoots").with("path", raw),
            )),
        }
    }
}

/// A path for display: Windows verbatim prefixes (`\\?\C:\`) stripped.
pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_owned(),
        _ => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[test]
    fn display_strips_verbatim_prefix() {
        assert_eq!(display_path(Path::new(r"\\?\C:\Games\CD")), r"C:\Games\CD");
        assert_eq!(display_path(Path::new(r"\\?\UNC\nas\ps2")), r"\\nas\ps2");
        assert_eq!(display_path(Path::new("/mnt/ps2")), "/mnt/ps2");
    }

    #[test]
    fn allow_list() {
        let dir = std::env::temp_dir();
        let root = std::fs::canonicalize(&dir).unwrap();
        let raw = dir.to_string_lossy();
        assert!(PathPolicy::Any.resolve(&raw).is_ok());
        assert!(PathPolicy::Within(vec![root.clone()]).resolve(&raw).is_ok());
        let err = PathPolicy::Within(Vec::new()).resolve(&raw).unwrap_err();
        assert_eq!(err.code, ErrorCode::Forbidden);
        let err = PathPolicy::Any.resolve("definitely/not/here").unwrap_err();
        assert_eq!(err.code, ErrorCode::NotFound);
        let err = PathPolicy::Any.resolve("").unwrap_err();
        assert_eq!(err.code, ErrorCode::BadRequest);
    }
}
