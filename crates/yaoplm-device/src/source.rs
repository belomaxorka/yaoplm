//! Read access to import sources on the host (IMP-1, IMP-11).
//!
//! Sources live outside the device root, so they are never written. The
//! one exception is [`SourceFs::remove`], which a plan calls only when the
//! user explicitly chose to move a game rather than copy it. Tests swap in
//! generated multi-gigabyte images without touching the disk.

use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMeta {
    pub is_dir: bool,
    pub len: u64,
    /// Modification time in nanoseconds since 1970, when known: a changed
    /// source between plan and execution is refused.
    pub modified: Option<u128>,
}

pub trait SourceFs: Send + Sync {
    fn metadata(&self, path: &Path) -> io::Result<SourceMeta>;
    /// Entries of a folder, sorted by name.
    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
    fn open(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>>;
    /// Deletes a source after it was moved to the device.
    fn remove(&self, path: &Path) -> io::Result<()>;
}

/// The host file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostSources;

impl SourceFs for HostSources {
    fn metadata(&self, path: &Path) -> io::Result<SourceMeta> {
        let meta = fs::metadata(path)?;
        Ok(SourceMeta {
            is_dir: meta.is_dir(),
            len: meta.len(),
            modified: meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_nanos()),
        })
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut entries = fs::read_dir(dir)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<io::Result<Vec<_>>>()?;
        entries.sort();
        Ok(entries)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        Ok(Box::new(fs::File::open(path)?))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
}
