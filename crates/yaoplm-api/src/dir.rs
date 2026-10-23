//! Folder listings for the web UI's source picker (IMP-1, ADR-0009).
//!
//! A browser cannot hand the server a file path, so the UI browses the
//! server's folders instead. Listings go through the same [`PathPolicy`] as
//! every other request: the server shows its library roots and what is
//! inside them, never the rest of the host.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ApiError;
use crate::paths::{PathPolicy, display_path};

/// Entries beyond this many are dropped and `truncated` is set.
pub const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    Dir,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    pub name: String,
    /// Absolute, for the next `ListDir` or as an import source.
    pub path: String,
    pub kind: EntryKind,
    pub size: Option<u64>,
    /// An image, a sheet or an archive the importer takes (IMP-1, IMP-1b).
    pub importable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DirListing {
    /// `None` for the top level: the library roots, or the volumes.
    pub path: Option<String>,
    /// The folder above, when the policy allows going there.
    pub parent: Option<String>,
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

/// Extensions the importer accepts (`yaoplm_core::import`); zip and 7z
/// archives open like folders (IMP-1b).
pub fn is_importable(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "iso" | "zso" | "bin" | "cue" | "chd" | "zip" | "7z"
            )
        })
}

fn root_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| display_path(path))
}

/// The top level: library roots on the server, volumes on the desktop.
pub fn list_top(policy: &PathPolicy) -> DirListing {
    let entries = match policy {
        PathPolicy::Within(roots) => roots
            .iter()
            .map(|root| DirEntry {
                name: root_name(root),
                path: display_path(root),
                kind: EntryKind::Dir,
                size: None,
                importable: false,
            })
            .collect(),
        PathPolicy::Any => yaoplm_device::list_volumes()
            .iter()
            .map(|volume| DirEntry {
                name: match &volume.label {
                    Some(label) if !label.is_empty() => {
                        format!("{label} ({})", display_path(&volume.root))
                    }
                    _ => display_path(&volume.root),
                },
                path: display_path(&volume.root),
                kind: EntryKind::Dir,
                size: None,
                importable: false,
            })
            .collect(),
    };
    DirListing {
        path: None,
        parent: None,
        entries,
        truncated: false,
    }
}

/// Lists `raw`, which the policy must allow.
pub fn list_dir(policy: &PathPolicy, raw: &str) -> Result<DirListing, ApiError> {
    let dir = policy.resolve(raw)?;
    if !dir.is_dir() {
        return Err(ApiError::bad_request(format!(
            "{} is not a folder",
            display_path(&dir)
        )));
    }
    let read = fs::read_dir(&dir)
        .map_err(|err| ApiError::internal(format!("cannot list {}: {err}", display_path(&dir))))?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        // Hidden files, `.yaoplm/` and macOS `._*` leftovers are never sources.
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        // A symlink could point outside the allowed roots; `resolve` would
        // refuse it later, so do not offer it at all.
        if kind.is_symlink() {
            continue;
        }
        let (kind, size) = if kind.is_dir() {
            (EntryKind::Dir, None)
        } else {
            (EntryKind::File, entry.metadata().ok().map(|m| m.len()))
        };
        if entries.len() >= MAX_ENTRIES {
            truncated = true;
            break;
        }
        entries.push(DirEntry {
            importable: kind == EntryKind::File && is_importable(&name),
            path: display_path(&dir.join(&name)),
            name,
            kind,
            size,
        });
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir)
            .cmp(&(b.kind != EntryKind::Dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(DirListing {
        parent: parent_of(policy, &dir),
        path: Some(display_path(&dir)),
        entries,
        truncated,
    })
}

/// The folder above `dir`, unless that leaves the allowed roots.
fn parent_of(policy: &PathPolicy, dir: &Path) -> Option<String> {
    let parent: PathBuf = dir.parent()?.to_path_buf();
    match policy {
        PathPolicy::Any => Some(display_path(&parent)),
        PathPolicy::Within(roots) if roots.iter().any(|root| parent.starts_with(root)) => {
            Some(display_path(&parent))
        }
        PathPolicy::Within(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importable_extensions() {
        for name in [
            "Game.iso", "game.ZSO", "disc.bin", "disc.Cue", "game.chd", "dump.zip", "dump.7z",
        ] {
            assert!(is_importable(name), "{name}");
        }
        for name in ["game.rar", "readme.txt", "iso", "folder", ".iso"] {
            assert!(!is_importable(name), "{name}");
        }
    }
}
