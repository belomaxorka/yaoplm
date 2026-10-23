//! Import sources inside zip and 7z archives (IMP-1b).
//!
//! [`ArchiveSources`] wraps the host's [`SourceFs`]: an archive reads as a
//! folder and its members as files, under paths like
//! `D:\Games\Ico.7z\Ico (USA).iso`. Everything else in the importer (the
//! folder walk, CUE sheets naming their BINs, the probe, the copy) works on
//! them unchanged. Members are never deleted: moving a game out of an
//! archive leaves the archive alone, and the plan says so.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use yaoplm_device::source::{ReadSeek, SourceFs, SourceMeta};
use yaoplm_formats::archive::{self, Entry, Kind};

type Listing = Arc<Vec<Entry>>;

/// A [`SourceFs`] that opens zip and 7z archives like folders.
pub struct ArchiveSources<'a> {
    inner: &'a dyn SourceFs,
    /// Listings by archive path, with the archive's time when listed.
    listings: Mutex<HashMap<PathBuf, (Option<u128>, Listing)>>,
}

impl std::fmt::Debug for ArchiveSources<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchiveSources").finish_non_exhaustive()
    }
}

/// An archive, and a path inside it (`/`-separated, empty for its root).
struct Located {
    archive: PathBuf,
    kind: Kind,
    inner: String,
}

fn unsupported(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, what.to_owned())
}

impl<'a> ArchiveSources<'a> {
    pub fn new(inner: &'a dyn SourceFs) -> Self {
        Self {
            inner,
            listings: Mutex::new(HashMap::new()),
        }
    }

    /// The archive `path` is, or is in. The nearest ancestor that is a file
    /// with an archive extension; archives inside archives are not opened.
    fn locate(&self, path: &Path) -> Option<Located> {
        let mut current = Some(path);
        while let Some(candidate) = current {
            if let Some(kind) = candidate
                .extension()
                .and_then(|e| e.to_str())
                .and_then(Kind::from_extension)
                && self.inner.metadata(candidate).is_ok_and(|m| !m.is_dir)
            {
                let rest = path.strip_prefix(candidate).ok()?;
                let parts: Vec<String> = rest
                    .components()
                    .filter_map(|c| match c {
                        Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                        _ => None,
                    })
                    .collect();
                return Some(Located {
                    archive: candidate.to_path_buf(),
                    kind,
                    inner: parts.join("/"),
                });
            }
            current = candidate.parent();
        }
        None
    }

    fn listing(&self, archive: &Path, kind: Kind) -> io::Result<(SourceMeta, Listing)> {
        let meta = self.inner.metadata(archive)?;
        let mut listings = self.listings.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((modified, listing)) = listings.get(archive)
            && *modified == meta.modified
        {
            return Ok((meta, Arc::clone(listing)));
        }
        let mut file = self.inner.open(archive)?;
        let entries = archive::list(kind, &mut file).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", archive.display()),
            )
        })?;
        let listing = Arc::new(entries);
        listings.insert(archive.to_path_buf(), (meta.modified, Arc::clone(&listing)));
        Ok((meta, listing))
    }

    fn find<'l>(listing: &'l [Entry], name: &str) -> Option<&'l Entry> {
        listing
            .iter()
            .find(|e| e.name == name)
            .or_else(|| listing.iter().find(|e| e.name.eq_ignore_ascii_case(name)))
    }
}

impl SourceFs for ArchiveSources<'_> {
    fn metadata(&self, path: &Path) -> io::Result<SourceMeta> {
        let Some(at) = self.locate(path) else {
            return self.inner.metadata(path);
        };
        let (meta, listing) = self.listing(&at.archive, at.kind)?;
        if at.inner.is_empty() {
            return Ok(SourceMeta {
                is_dir: true,
                len: 0,
                modified: meta.modified,
            });
        }
        let entry = Self::find(&listing, &at.inner).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not in the archive", at.inner),
            )
        })?;
        Ok(SourceMeta {
            is_dir: entry.is_dir,
            len: entry.size,
            // A member changes when its archive does.
            modified: meta.modified,
        })
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let Some(at) = self.locate(dir) else {
            return self.inner.list(dir);
        };
        let (_, listing) = self.listing(&at.archive, at.kind)?;
        let prefix = if at.inner.is_empty() {
            String::new()
        } else {
            format!("{}/", at.inner)
        };
        let mut children: Vec<PathBuf> = listing
            .iter()
            .filter_map(|e| {
                let rest = e.name.strip_prefix(&prefix)?;
                (!rest.is_empty() && !rest.contains('/')).then(|| dir.join(rest))
            })
            .collect();
        children.sort();
        Ok(children)
    }

    fn open(&self, path: &Path) -> io::Result<Box<dyn ReadSeek>> {
        let Some(at) = self.locate(path) else {
            return self.inner.open(path);
        };
        let (_, listing) = self.listing(&at.archive, at.kind)?;
        let name = Self::find(&listing, &at.inner)
            .map(|e| e.name.clone())
            .unwrap_or(at.inner);
        let file = self.inner.open(&at.archive)?;
        let reader = archive::open_entry(at.kind, Box::new(file), &name).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", at.archive.display()),
            )
        })?;
        Ok(Box::new(reader))
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        match self.locate(path) {
            None => self.inner.remove(path),
            Some(_) => Err(unsupported("files inside an archive are not deleted")),
        }
    }

    fn in_archive(&self, path: &Path) -> bool {
        self.locate(path).is_some_and(|at| !at.inner.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use yaoplm_device::HostSources;

    use super::*;

    #[test]
    fn archives_read_as_folders() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("Games.zip");
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            zip.add_directory("Disc", options).unwrap();
            zip.start_file("Disc/Game.iso", options).unwrap();
            std::io::Write::write_all(&mut zip, &[7u8; 5000]).unwrap();
            zip.start_file("readme.txt", options).unwrap();
            zip.finish().unwrap();
        }
        std::fs::write(&zip_path, buf.into_inner()).unwrap();
        let sources = ArchiveSources::new(&HostSources);

        let root = sources.metadata(&zip_path).unwrap();
        assert!(root.is_dir);
        assert_eq!(
            sources.list(&zip_path).unwrap(),
            [zip_path.join("Disc"), zip_path.join("readme.txt")]
        );
        let game = zip_path.join("Disc").join("Game.iso");
        assert_eq!(
            sources.list(&zip_path.join("Disc")).unwrap(),
            [game.as_path()]
        );
        let meta = sources.metadata(&game).unwrap();
        assert!(!meta.is_dir);
        assert_eq!(meta.len, 5000);
        assert!(sources.in_archive(&game));
        assert!(!sources.in_archive(&zip_path));
        let mut bytes = Vec::new();
        sources
            .open(&game)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, [7u8; 5000]);
        assert_eq!(
            sources.remove(&game).unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
        assert!(sources.metadata(&zip_path.join("missing.iso")).is_err());
        // Plain files pass through.
        assert!(sources.metadata(dir.path()).unwrap().is_dir);
    }
}
