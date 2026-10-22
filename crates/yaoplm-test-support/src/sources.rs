//! Import sources that exist only in memory: generated ISOs of any size,
//! BIN/CUE pairs, small byte files.

use std::collections::BTreeMap;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use yaoplm_device::source::{ReadSeek, SourceFs, SourceMeta};

use crate::content::{Content, VirtualFile};
use crate::iso::VirtualIso;

#[derive(Debug, Clone)]
enum Source {
    Iso(VirtualIso),
    File(Content),
}

/// A host directory tree made of generated files.
#[derive(Debug, Default)]
pub struct VirtualSources {
    files: Mutex<BTreeMap<PathBuf, (Source, u128)>>,
    removed: Mutex<Vec<PathBuf>>,
}

impl VirtualSources {
    pub fn new() -> Self {
        Self::default()
    }

    fn files(&self) -> MutexGuard<'_, BTreeMap<PathBuf, (Source, u128)>> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn add_iso(&self, path: impl Into<PathBuf>, iso: VirtualIso) -> &Self {
        self.files().insert(path.into(), (Source::Iso(iso), 1));
        self
    }

    pub fn add_file(&self, path: impl Into<PathBuf>, content: Content) -> &Self {
        self.files().insert(path.into(), (Source::File(content), 1));
        self
    }

    /// Pretends the file was modified after the plan was made.
    pub fn touch(&self, path: &Path) {
        if let Some(entry) = self.files().get_mut(path) {
            entry.1 += 1;
        }
    }

    /// Paths deleted through [`SourceFs::remove`].
    pub fn removed(&self) -> Vec<PathBuf> {
        self.removed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} not found", path.display()),
    )
}

impl SourceFs for VirtualSources {
    fn metadata(&self, path: &Path) -> io::Result<SourceMeta> {
        let files = self.files();
        if let Some((source, modified)) = files.get(path) {
            let len = match source {
                Source::Iso(iso) => iso.len(),
                Source::File(content) => content.len(),
            };
            return Ok(SourceMeta {
                is_dir: false,
                len,
                modified: Some(*modified),
            });
        }
        if files.keys().any(|p| p.starts_with(path)) {
            return Ok(SourceMeta {
                is_dir: true,
                len: 0,
                modified: None,
            });
        }
        Err(not_found(path))
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let files = self.files();
        let mut children: Vec<PathBuf> = files
            .keys()
            .filter_map(|p| {
                let rest = p.strip_prefix(dir).ok()?;
                let first = rest.components().next()?;
                Some(dir.join(first))
            })
            .collect();
        if children.is_empty() {
            return Err(not_found(dir));
        }
        children.sort();
        children.dedup();
        Ok(children)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        match self.files().get(path) {
            Some((Source::Iso(iso), _)) => Ok(Box::new(iso.clone())),
            Some((Source::File(Content::Bytes(bytes)), _)) => {
                Ok(Box::new(Cursor::new(bytes.clone())))
            }
            Some((Source::File(content), _)) => Ok(Box::new(VirtualFile::new(content.clone()))),
            None => Err(not_found(path)),
        }
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.files().remove(path).ok_or_else(|| not_found(path))?;
        self.removed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path.to_path_buf());
        Ok(())
    }
}
