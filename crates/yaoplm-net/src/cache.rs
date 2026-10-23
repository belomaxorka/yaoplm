//! The download cache (ARCHITECTURE.md §6, research/03 §7.2).
//!
//! Layout under the app data folder:
//!
//! ```text
//! art-cache/<source>/<key>/original.<ext>    what the source sent
//! art-cache/<source>/<key>/COV-140x200.png   derived OPL-ready PNGs
//! cache.sqlite                               the manifest
//! ```
//!
//! The manifest row per object keeps the URL, its validators (`ETag`,
//! `Last-Modified`), when it was fetched, the SHA-256 of the original and
//! the licence tag and attribution of the source. "Not found" is recorded
//! too, so a missing cover is not asked for again for seven days. The whole
//! folder can be deleted without losing anything: the device holds the
//! truth, the cache only saves network round trips.
//!
//! The same manifest keeps the hash cache of dump verification (GAME-5):
//! hashes of an image keyed by its files' paths, sizes and modification
//! times, so a game is read in full only once until it changes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use yaoplm_hash::Hashes;
use yaoplm_hash::digest::{hex, parse_hex};

use crate::NetError;
use crate::http::Validators;

pub const ART_DIR: &str = "art-cache";
pub const MANIFEST: &str = "cache.sqlite";
/// How long a "not found" answer is believed.
pub const MISSING_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Missing,
}

/// A manifest row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub source: String,
    pub key: String,
    pub url: String,
    pub status: Status,
    /// File name of the original inside the key folder.
    pub file: Option<String>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Seconds since the Unix epoch.
    pub fetched_at: u64,
    pub sha256: Option<String>,
    pub size: Option<u64>,
    pub license_tag: String,
    pub attribution: String,
}

impl Record {
    pub fn validators(&self) -> Validators {
        Validators {
            etag: self.etag.clone(),
            last_modified: self.last_modified.clone(),
        }
    }

    /// A "not found" older than [`MISSING_TTL`] is worth asking again.
    pub fn is_stale_missing(&self, now: u64) -> bool {
        self.status == Status::Missing
            && now.saturating_sub(self.fetched_at) > MISSING_TTL.as_secs()
    }
}

/// What a source hands the cache after a successful download.
#[derive(Debug, Clone)]
pub struct Download<'a> {
    pub url: &'a str,
    pub bytes: &'a [u8],
    pub content_type: Option<&'a str>,
    pub etag: Option<&'a str>,
    pub last_modified: Option<&'a str>,
    pub license_tag: &'a str,
    pub attribution: &'a str,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A folder name that is safe on FAT, NTFS and ext4: ASCII letters,
/// digits and a few punctuation marks, at most 100 characters, with a
/// short hash when anything was changed so distinct keys stay distinct.
pub fn safe_key(key: &str) -> String {
    let mut out: String = key
        .chars()
        .map(|c| match c {
            'A'..='Z'
            | 'a'..='z'
            | '0'..='9'
            | '-'
            | '_'
            | '.'
            | ' '
            | '('
            | ')'
            | ','
            | '+'
            | '!'
            | '\''
            | '&' => c,
            _ => '_',
        })
        .collect();
    let changed = out != key || key.len() > 100;
    if out.len() > 100 {
        let mut cut = 100;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    let out = out.trim_end_matches([' ', '.']).to_owned();
    if changed || out.is_empty() {
        format!("{out}~{}", &sha256_hex(key.as_bytes())[..8])
    } else {
        out
    }
}

/// File extension for a downloaded body, from the content type or the
/// bytes themselves.
fn extension_for(content_type: Option<&str>, bytes: &[u8]) -> &'static str {
    let mime = content_type
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match mime.as_str() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/gif" => "gif",
        _ => match bytes {
            [0x89, b'P', b'N', b'G', ..] => "png",
            [0xFF, 0xD8, 0xFF, ..] => "jpg",
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'E',
                b'B',
                b'P',
                ..,
            ] => "webp",
            [b'B', b'M', ..] => "bmp",
            [b'G', b'I', b'F', b'8', ..] => "gif",
            _ => "bin",
        },
    }
}

pub struct Cache {
    root: PathBuf,
    db: Mutex<Connection>,
}

impl std::fmt::Debug for Cache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cache").field("root", &self.root).finish()
    }
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS objects (
    source        TEXT NOT NULL,
    key           TEXT NOT NULL,
    url           TEXT NOT NULL,
    status        TEXT NOT NULL,
    file          TEXT,
    content_type  TEXT,
    etag          TEXT,
    last_modified TEXT,
    fetched_at    INTEGER NOT NULL,
    sha256        TEXT,
    size          INTEGER,
    license_tag   TEXT NOT NULL DEFAULT '',
    attribution   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (source, key)
);
CREATE TABLE IF NOT EXISTS hashes (
    files     TEXT PRIMARY KEY,
    size      INTEGER NOT NULL,
    crc32     TEXT NOT NULL,
    sha1      TEXT NOT NULL,
    md5       TEXT,
    hashed_at INTEGER NOT NULL
);
";

/// One file an image was read from, as it was when hashed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashedFile<'a> {
    pub path: &'a Path,
    pub len: u64,
    /// Nanoseconds since 1970; without it nothing is cached.
    pub modified: Option<u128>,
}

/// The cache key of an image: every file with its size and time.
fn files_key(files: &[HashedFile<'_>]) -> Option<String> {
    let mut parts = Vec::with_capacity(files.len());
    for file in files {
        parts.push(format!(
            "{}\u{1f}{}\u{1f}{}",
            file.path.display(),
            file.len,
            file.modified?
        ));
    }
    (!parts.is_empty()).then(|| parts.join("\u{1e}"))
}

impl Cache {
    /// Opens (or creates) the cache in `data_dir`.
    pub fn open(data_dir: &Path) -> Result<Self, NetError> {
        fs::create_dir_all(data_dir.join(ART_DIR)).map_err(NetError::io(data_dir))?;
        let db = Connection::open(data_dir.join(MANIFEST))?;
        db.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        db.execute_batch(SCHEMA)?;
        Ok(Self {
            root: data_dir.to_path_buf(),
            db: Mutex::new(db),
        })
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `art-cache/<source>/<key>`.
    pub fn object_dir(&self, source: &str, key: &str) -> PathBuf {
        self.root
            .join(ART_DIR)
            .join(safe_key(source))
            .join(safe_key(key))
    }

    pub fn get(&self, source: &str, key: &str) -> Result<Option<Record>, NetError> {
        let db = self.db();
        let row = db
            .query_row(
                "SELECT source, key, url, status, file, content_type, etag, last_modified, fetched_at, sha256, size, license_tag, attribution FROM objects WHERE source = ?1 AND key = ?2",
                params![source, key],
                |row| {
                    Ok(Record {
                        source: row.get(0)?,
                        key: row.get(1)?,
                        url: row.get(2)?,
                        status: if row.get::<_, String>(3)? == "ok" {
                            Status::Ok
                        } else {
                            Status::Missing
                        },
                        file: row.get(4)?,
                        content_type: row.get(5)?,
                        etag: row.get(6)?,
                        last_modified: row.get(7)?,
                        fetched_at: row.get::<_, i64>(8).map(|v| v.max(0) as u64)?,
                        sha256: row.get(9)?,
                        size: row.get::<_, Option<i64>>(10)?.map(|v| v.max(0) as u64),
                        license_tag: row.get(11)?,
                        attribution: row.get(12)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// The path of a cached original, if the record has one and the file
    /// is still there.
    pub fn original_path(&self, record: &Record) -> Option<PathBuf> {
        let file = record.file.as_ref()?;
        let path = self.object_dir(&record.source, &record.key).join(file);
        path.is_file().then_some(path)
    }

    /// Stores a downloaded original and its manifest row. Derived files of
    /// an earlier download are dropped: they were made from other bytes.
    pub fn put(
        &self,
        source: &str,
        key: &str,
        download: &Download<'_>,
    ) -> Result<Record, NetError> {
        let dir = self.object_dir(source, key);
        fs::create_dir_all(&dir).map_err(NetError::io(&dir))?;
        let ext = extension_for(download.content_type, download.bytes);
        let file = format!("original.{ext}");
        let sha = sha256_hex(download.bytes);
        let changed = self
            .get(source, key)?
            .is_none_or(|old| old.sha256.as_deref() != Some(sha.as_str()));
        if changed {
            clear_derived(&dir)?;
        }
        write_atomic(&dir.join(&file), download.bytes)?;
        let record = Record {
            source: source.to_owned(),
            key: key.to_owned(),
            url: download.url.to_owned(),
            status: Status::Ok,
            file: Some(file),
            content_type: download.content_type.map(str::to_owned),
            etag: download.etag.map(str::to_owned),
            last_modified: download.last_modified.map(str::to_owned),
            fetched_at: now_secs(),
            sha256: Some(sha),
            size: Some(download.bytes.len() as u64),
            license_tag: download.license_tag.to_owned(),
            attribution: download.attribution.to_owned(),
        };
        self.upsert(&record)?;
        Ok(record)
    }

    /// Records that `url` has nothing for `key` (404).
    pub fn put_missing(&self, source: &str, key: &str, url: &str) -> Result<Record, NetError> {
        let record = Record {
            source: source.to_owned(),
            key: key.to_owned(),
            url: url.to_owned(),
            status: Status::Missing,
            file: None,
            content_type: None,
            etag: None,
            last_modified: None,
            fetched_at: now_secs(),
            sha256: None,
            size: None,
            license_tag: String::new(),
            attribution: String::new(),
        };
        self.upsert(&record)?;
        Ok(record)
    }

    /// Refreshes `fetched_at` after a 304.
    pub fn touch(&self, record: &Record) -> Result<Record, NetError> {
        let mut fresh = record.clone();
        fresh.fetched_at = now_secs();
        self.upsert(&fresh)?;
        Ok(fresh)
    }

    fn upsert(&self, r: &Record) -> Result<(), NetError> {
        self.db().execute(
            "INSERT INTO objects (source, key, url, status, file, content_type, etag, last_modified, fetched_at, sha256, size, license_tag, attribution)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(source, key) DO UPDATE SET url = excluded.url, status = excluded.status, file = excluded.file,
               content_type = excluded.content_type, etag = excluded.etag, last_modified = excluded.last_modified,
               fetched_at = excluded.fetched_at, sha256 = excluded.sha256, size = excluded.size,
               license_tag = excluded.license_tag, attribution = excluded.attribution",
            params![
                r.source,
                r.key,
                r.url,
                match r.status {
                    Status::Ok => "ok",
                    Status::Missing => "missing",
                },
                r.file,
                r.content_type,
                r.etag,
                r.last_modified,
                i64::try_from(r.fetched_at).unwrap_or(i64::MAX),
                r.sha256,
                r.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
                r.license_tag,
                r.attribution,
            ],
        )?;
        Ok(())
    }

    /// A derived file next to the original: `<label>.png`, made by `make`
    /// from the original bytes when it does not exist yet.
    pub fn derived(
        &self,
        record: &Record,
        label: &str,
        make: impl FnOnce(&[u8]) -> Result<Vec<u8>, NetError>,
    ) -> Result<PathBuf, NetError> {
        let dir = self.object_dir(&record.source, &record.key);
        let path = dir.join(format!("{}.png", safe_key(label)));
        if path.is_file() {
            return Ok(path);
        }
        let original = self.original_path(record).ok_or_else(|| {
            NetError::Cache(format!(
                "{}/{}: original is gone",
                record.source, record.key
            ))
        })?;
        let bytes = fs::read(&original).map_err(NetError::io(&original))?;
        let derived = make(&bytes)?;
        write_atomic(&path, &derived)?;
        Ok(path)
    }

    /// Hashes of an image whose files have not changed since they were
    /// hashed. With `md5`, a row without MD5 does not count.
    pub fn hashes(&self, files: &[HashedFile<'_>], md5: bool) -> Result<Option<Hashes>, NetError> {
        let Some(key) = files_key(files) else {
            return Ok(None);
        };
        let row = self
            .db()
            .query_row(
                "SELECT size, crc32, sha1, md5 FROM hashes WHERE files = ?1",
                params![key],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((size, crc32, sha1, md5_hex)) = row else {
            return Ok(None);
        };
        let md5_value = md5_hex.as_deref().and_then(parse_hex::<16>);
        if md5 && md5_value.is_none() {
            return Ok(None);
        }
        Ok(parse_hex::<4>(&crc32)
            .zip(parse_hex::<20>(&sha1))
            .map(|(crc32, sha1)| Hashes {
                size: size.max(0) as u64,
                crc32: u32::from_be_bytes(crc32),
                sha1,
                md5: md5_value,
            }))
    }

    /// Remembers the hashes of an image read from `files`.
    pub fn put_hashes(&self, files: &[HashedFile<'_>], hashes: &Hashes) -> Result<(), NetError> {
        let Some(key) = files_key(files) else {
            return Ok(());
        };
        self.db().execute(
            "INSERT INTO hashes (files, size, crc32, sha1, md5, hashed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(files) DO UPDATE SET size = excluded.size, crc32 = excluded.crc32,
               sha1 = excluded.sha1, md5 = excluded.md5, hashed_at = excluded.hashed_at",
            params![
                key,
                i64::try_from(hashes.size).unwrap_or(i64::MAX),
                hashes.crc32_hex(),
                hex(&hashes.sha1),
                hashes.md5_hex(),
                i64::try_from(now_secs()).unwrap_or(i64::MAX),
            ],
        )?;
        Ok(())
    }

    /// Deletes every cached object and manifest row.
    pub fn clear(&self) -> Result<(), NetError> {
        self.db().execute("DELETE FROM objects", [])?;
        let dir = self.root.join(ART_DIR);
        if dir.is_dir() {
            fs::remove_dir_all(&dir).map_err(NetError::io(&dir))?;
        }
        fs::create_dir_all(&dir).map_err(NetError::io(&dir))?;
        Ok(())
    }

    /// Rows and bytes on disk, for the settings screen.
    pub fn stats(&self) -> Result<(u64, u64), NetError> {
        let rows: i64 = self
            .db()
            .query_row("SELECT COUNT(*) FROM objects", [], |r| r.get(0))?;
        let bytes = dir_size(&self.root.join(ART_DIR)).unwrap_or(0);
        Ok((rows.max(0) as u64, bytes))
    }
}

fn clear_derived(dir: &Path) -> Result<(), NetError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("original.") {
            let path = entry.path();
            fs::remove_file(&path).map_err(NetError::io(&path))?;
        }
    }
    Ok(())
}

fn dir_size(dir: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        total += if meta.is_dir() {
            dir_size(&entry.path())?
        } else {
            meta.len()
        };
    }
    Ok(total)
}

/// Writes through a temporary file so a crash never leaves a half file
/// that looks like a cached original.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), NetError> {
    let tmp = path.with_extension("part");
    fs::write(&tmp, bytes).map_err(NetError::io(&tmp))?;
    fs::rename(&tmp, path).map_err(NetError::io(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_negative_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        assert!(cache.get("xlenore", "SLUS-20062").unwrap().is_none());
        let record = cache
            .put(
                "xlenore",
                "SLUS-20062",
                &Download {
                    url: "https://example/SLUS-20062.jpg",
                    bytes: &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3],
                    content_type: Some("image/jpeg"),
                    etag: Some("\"abc\""),
                    last_modified: None,
                    license_tag: "none",
                    attribution: "xlenore",
                },
            )
            .unwrap();
        assert_eq!(record.file.as_deref(), Some("original.jpg"));
        assert_eq!(record.status, Status::Ok);
        assert!(cache.original_path(&record).unwrap().is_file());
        let again = cache.get("xlenore", "SLUS-20062").unwrap().unwrap();
        assert_eq!(again, record);
        assert_eq!(again.validators().etag.as_deref(), Some("\"abc\""));

        let derived = cache
            .derived(&record, "COV-140x200", |bytes| Ok(bytes.to_vec()))
            .unwrap();
        assert!(derived.ends_with("COV-140x200.png"));
        // A second call reuses the file instead of calling `make`.
        let same = cache
            .derived(&record, "COV-140x200", |_| panic!("not called"))
            .unwrap();
        assert_eq!(same, derived);
        // New bytes drop the derived files.
        cache
            .put(
                "xlenore",
                "SLUS-20062",
                &Download {
                    url: "https://example/SLUS-20062.jpg",
                    bytes: &[0x89, b'P', b'N', b'G', 9],
                    content_type: None,
                    etag: None,
                    last_modified: None,
                    license_tag: "none",
                    attribution: "xlenore",
                },
            )
            .unwrap();
        assert!(!derived.exists());
        assert_eq!(
            cache
                .get("xlenore", "SLUS-20062")
                .unwrap()
                .unwrap()
                .file
                .as_deref(),
            Some("original.png")
        );

        let missing = cache
            .put_missing("libretro", "Nope (USA)", "https://example/x")
            .unwrap();
        assert_eq!(missing.status, Status::Missing);
        assert!(!missing.is_stale_missing(now_secs()));
        assert!(missing.is_stale_missing(now_secs() + MISSING_TTL.as_secs() + 1));
        let (rows, bytes) = cache.stats().unwrap();
        assert_eq!(rows, 2);
        assert!(bytes > 0);
        cache.clear().unwrap();
        assert_eq!(cache.stats().unwrap().0, 0);
        assert!(cache.get("xlenore", "SLUS-20062").unwrap().is_none());
    }

    #[test]
    fn hashes_are_keyed_by_files_and_their_times() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::open(dir.path()).unwrap();
        let path = Path::new("E:/DVD/SLUS_200.62.Game.iso");
        let file = HashedFile {
            path,
            len: 4096,
            modified: Some(17),
        };
        let hashes = Hashes {
            size: 4096,
            crc32: 0x0102_0304,
            sha1: [7; 20],
            md5: None,
        };
        assert_eq!(cache.hashes(&[file], false).unwrap(), None);
        cache.put_hashes(&[file], &hashes).unwrap();
        assert_eq!(cache.hashes(&[file], false).unwrap(), Some(hashes));
        // MD5 was not computed: a run that wants it reads the image again.
        assert_eq!(cache.hashes(&[file], true).unwrap(), None);
        let with_md5 = Hashes {
            md5: Some([9; 16]),
            ..hashes
        };
        cache.put_hashes(&[file], &with_md5).unwrap();
        assert_eq!(cache.hashes(&[file], true).unwrap(), Some(with_md5));
        // A touched file is a new key.
        let touched = HashedFile {
            modified: Some(18),
            ..file
        };
        assert_eq!(cache.hashes(&[touched], false).unwrap(), None);
        // Without a time nothing is cached.
        let timeless = HashedFile {
            modified: None,
            ..file
        };
        cache.put_hashes(&[timeless], &hashes).unwrap();
        assert_eq!(cache.hashes(&[timeless], false).unwrap(), None);
        // Clearing the covers keeps the hashes.
        cache.clear().unwrap();
        assert!(cache.hashes(&[file], true).unwrap().is_some());
    }

    #[test]
    fn keys_are_file_system_safe() {
        assert_eq!(safe_key("SLUS-20062"), "SLUS-20062");
        assert_eq!(
            safe_key("Grand Theft Auto III (USA, Canada)"),
            "Grand Theft Auto III (USA, Canada)"
        );
        let odd = safe_key("Pok\u{e9}mon: A/B?");
        assert!(odd.starts_with("Pok_mon_ A_B_~"), "{odd}");
        assert_ne!(safe_key("a/b"), safe_key("a?b"));
        assert!(safe_key(&"x".repeat(300)).len() <= 110);
        assert_eq!(safe_key("..."), format!("~{}", &sha256_hex(b"...")[..8]));
        assert_eq!(
            extension_for(Some("image/jpeg; charset=binary"), b""),
            "jpg"
        );
        assert_eq!(extension_for(None, &[0x89, b'P', b'N', b'G']), "png");
        assert_eq!(extension_for(Some("text/html"), b"<html>"), "bin");
    }
}
