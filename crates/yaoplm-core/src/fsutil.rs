//! File-system helpers shared by the scanner.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One directory listing with case-insensitive lookup. FAT32, exFAT and
/// Samba shares match names case-insensitively, so OPL finds `cd/` when it
/// asks for `CD/`; a plain Linux folder would not.
#[derive(Debug, Default)]
pub struct Listing {
    pub entries: Vec<Entry>,
    by_lower: HashMap<String, usize>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub len: u64,
}

impl Listing {
    /// Lists `dir`. A missing directory gives an empty listing.
    pub fn read(dir: &Path) -> io::Result<Self> {
        let mut listing = Self::default();
        let read = match fs::read_dir(dir) {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(listing),
            Err(err) => return Err(err),
        };
        for entry in read {
            let entry = entry?;
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let meta = entry.metadata()?;
            listing.entries.push(Entry {
                name,
                path: entry.path(),
                is_dir: meta.is_dir(),
                len: meta.len(),
            });
        }
        // Directory order differs between file systems; keep results stable.
        listing.entries.sort_by(|a, b| a.name.cmp(&b.name));
        for (i, entry) in listing.entries.iter().enumerate() {
            listing
                .by_lower
                .entry(entry.name.to_lowercase())
                .or_insert(i);
        }
        Ok(listing)
    }

    pub fn find(&self, name: &str) -> Option<&Entry> {
        self.by_lower
            .get(&name.to_lowercase())
            .map(|&i| &self.entries[i])
    }

    pub fn find_dir(&self, name: &str) -> Option<&Entry> {
        self.find(name).filter(|e| e.is_dir)
    }

    pub fn find_file(&self, name: &str) -> Option<&Entry> {
        self.find(name).filter(|e| !e.is_dir)
    }

    pub fn files(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.is_dir)
    }
}

/// Reads at most `max` bytes from the start of a file.
pub fn read_prefix(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    fs::File::open(path)?
        .take(max as u64)
        .read_to_end(&mut buf)?;
    Ok(buf)
}
