//! The Redump DAT for dump verification (META-2, ADR-0007).
//!
//! `https://redump.info/datfile/ps2/serial,version` answers with a zip of
//! one Logiqx `.dat` that carries serials (research/03 §3.1). Checked on
//! 2026-10-23: about 1.3 MB zipped, 5.5 MB unpacked, no `ETag` and no
//! `Last-Modified`, so a conditional request saves nothing. The rule is
//! therefore our own: Redump is asked at most once a week, whatever the
//! user clicks, and only when the user asks for an update or verifies with
//! no DAT at all. Without a network the user can import a DAT they
//! downloaded themselves (a `.dat` or the `.zip`), which is also how tests
//! and the e2e fixture get one.
//!
//! Layout under the app data folder (ARCHITECTURE.md §6):
//!
//! ```text
//! db/redump-ps2.dat    the Logiqx XML, unpacked
//! db/redump-ps2.json   where it came from and when ([`DatInfo`])
//! ```

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use yaoplm_hash::dat;

use crate::NetError;
use crate::cache::{now_secs, sha256_hex};
use crate::http::{Fetched, Http, Validators};

pub const DB_DIR: &str = "db";
pub const DAT_FILE: &str = "redump-ps2.dat";
pub const INFO_FILE: &str = "redump-ps2.json";
pub const URL: &str = "https://redump.info/datfile/ps2/serial,version";
/// Redump is asked at most this often.
pub const MIN_INTERVAL: Duration = Duration::from_secs(7 * 24 * 3600);
/// The limiter key of every Redump request.
const LIMITER: &str = "redump";
/// A PS2 DAT is 5.5 MB; anything far bigger is not one.
const MAX_DAT: u64 = 64 << 20;

/// What is known about the stored DAT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatInfo {
    /// The URL, or the file the user imported.
    pub source: String,
    /// The file name Redump gave it, e.g.
    /// `Sony - PlayStation 2 - Datfile (11845) (2026-10-23 01-56-22).dat`.
    pub file_name: Option<String>,
    /// `<header><description>`.
    pub description: String,
    /// `<header><version>`, the build date of the DAT.
    pub version: String,
    pub games: u64,
    /// The DAT has `<serial>` elements (the `serial,version` variant).
    pub serials: bool,
    pub sha256: String,
    /// Seconds since 1970: when this DAT was stored.
    pub stored_at: u64,
    /// Seconds since 1970: the last request to Redump, if any.
    pub checked_at: Option<u64>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
}

impl DatInfo {
    /// When Redump may be asked again.
    pub fn next_check(&self) -> u64 {
        self.checked_at.unwrap_or(0) + MIN_INTERVAL.as_secs()
    }
}

/// How an update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Redump was asked less than a week ago: no request was made.
    TooSoon(DatInfo),
    /// Redump said nothing changed (304).
    NotModified(DatInfo),
    /// A new DAT is stored.
    Stored(DatInfo),
}

impl Update {
    pub fn info(&self) -> &DatInfo {
        match self {
            Self::TooSoon(info) | Self::NotModified(info) | Self::Stored(info) => info,
        }
    }
}

/// The stored DAT and its metadata.
#[derive(Debug, Clone)]
pub struct RedumpStore {
    dir: PathBuf,
}

/// The `.dat` inside a zip, or the bytes themselves when they are not one.
/// Returns the member name for zips.
fn unpack(bytes: Vec<u8>) -> Result<(Vec<u8>, Option<String>), NetError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Ok((bytes, None));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| NetError::Dat(format!("not a readable zip: {e}")))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| NetError::Dat(format!("zip entry {index}: {e}")))?;
        let entry_name = entry.name().map(|n| n.into_owned()).unwrap_or_default();
        if !entry.is_file() || !entry_name.to_ascii_lowercase().ends_with(".dat") {
            continue;
        }
        if entry.size() > MAX_DAT {
            return Err(NetError::Dat(format!("{entry_name} is too large")));
        }
        let name = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
        let mut dat = Vec::new();
        (&mut entry)
            .take(MAX_DAT)
            .read_to_end(&mut dat)
            .map_err(|e| NetError::Dat(format!("cannot unpack the DAT: {e}")))?;
        return Ok((dat, name));
    }
    Err(NetError::Dat("the zip holds no .dat file".into()))
}

impl RedumpStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join(DB_DIR),
        }
    }

    pub fn dat_path(&self) -> PathBuf {
        self.dir.join(DAT_FILE)
    }

    fn info_path(&self) -> PathBuf {
        self.dir.join(INFO_FILE)
    }

    /// The stored DAT's metadata, `None` before the first download or
    /// import (or when the DAT itself is gone).
    pub fn info(&self) -> Result<Option<DatInfo>, NetError> {
        if !self.dat_path().is_file() {
            return Ok(None);
        }
        let path = self.info_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(NetError::io(&path)(err)),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| NetError::Dat(format!("{}: {e}", path.display())))
    }

    fn write_info(&self, info: &DatInfo) -> Result<(), NetError> {
        let json = serde_json::to_vec_pretty(info).map_err(|e| NetError::Dat(e.to_string()))?;
        write_atomic(&self.info_path(), &json)
    }

    /// Checks that `bytes` (a `.dat` or a zip with one) is a datafile and
    /// stores it.
    fn store(
        &self,
        bytes: Vec<u8>,
        source: &str,
        file_name: Option<String>,
        checked_at: Option<u64>,
        validators: Validators,
    ) -> Result<DatInfo, NetError> {
        let (dat, member) = unpack(bytes)?;
        let parsed = dat::parse(&dat).map_err(|e| NetError::Dat(e.to_string()))?;
        if parsed.games.is_empty() {
            return Err(NetError::Dat("the DAT lists no games".into()));
        }
        let info = DatInfo {
            source: source.to_owned(),
            file_name: member.or(file_name),
            description: parsed.header.description.clone(),
            version: parsed.header.version.clone(),
            games: parsed.games.len() as u64,
            serials: parsed.games.iter().any(|g| g.serial.is_some()),
            sha256: sha256_hex(&dat),
            stored_at: now_secs(),
            checked_at,
            etag: validators.etag,
            last_modified: validators.last_modified,
        };
        fs::create_dir_all(&self.dir).map_err(NetError::io(&self.dir))?;
        write_atomic(&self.dat_path(), &dat)?;
        self.write_info(&info)?;
        Ok(info)
    }

    /// Stores a DAT the user downloaded: a `.dat` or Redump's `.zip`. No
    /// network.
    pub fn import(&self, path: &Path) -> Result<DatInfo, NetError> {
        let meta = fs::metadata(path).map_err(NetError::io(path))?;
        if meta.len() > MAX_DAT {
            return Err(NetError::Dat(format!("{} is too large", path.display())));
        }
        let bytes = fs::read(path).map_err(NetError::io(path))?;
        let checked_at = self.info()?.and_then(|i| i.checked_at);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        self.store(
            bytes,
            &path.display().to_string(),
            name,
            checked_at,
            Validators::default(),
        )
    }

    /// Downloads a newer DAT unless Redump was asked less than a week ago.
    pub async fn update(&self, http: &Http) -> Result<Update, NetError> {
        let current = self.info()?;
        let now = now_secs();
        if let Some(info) = &current
            && now < info.next_check()
        {
            return Ok(Update::TooSoon(info.clone()));
        }
        let validators = current
            .as_ref()
            .map(|i| Validators {
                etag: i.etag.clone(),
                last_modified: i.last_modified.clone(),
            })
            .unwrap_or_default();
        match http.get_large(LIMITER, URL, &validators, MAX_DAT).await? {
            Fetched::NotModified => {
                let Some(mut info) = current else {
                    return Err(NetError::Dat("304 without a stored DAT".into()));
                };
                info.checked_at = Some(now);
                self.write_info(&info)?;
                Ok(Update::NotModified(info))
            }
            Fetched::NotFound => Err(NetError::Status {
                url: URL.to_owned(),
                status: 404,
            }),
            Fetched::Ok(body) => {
                let file_name = body.file_name.clone();
                let validators = Validators {
                    etag: body.etag,
                    last_modified: body.last_modified,
                };
                self.store(body.bytes, URL, file_name, Some(now), validators)
                    .map(Update::Stored)
            }
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), NetError> {
    let tmp = path.with_extension("part");
    fs::write(&tmp, bytes).map_err(NetError::io(&tmp))?;
    fs::rename(&tmp, path).map_err(NetError::io(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const DAT: &str = r#"<?xml version="1.0"?>
<datafile>
  <header><name>Sony - PlayStation 2</name><description>Sony - PlayStation 2 - Datfile (1) (2026-10-23 01-56-22)</description><version>2026-10-23 01-56-22</version></header>
  <game name="Game (USA)" id="1"><serial>SLUS-20062</serial>
    <rom name="Game (USA).iso" size="2048" crc="00000000" sha1="0000000000000000000000000000000000000000"/></game>
</datafile>"#;

    fn zipped(name: &str, body: &[u8]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(body).unwrap();
        zip.finish().unwrap();
        out.into_inner()
    }

    #[test]
    fn imports_a_dat_or_a_zip() {
        let data = tempfile::tempdir().unwrap();
        let store = RedumpStore::new(data.path());
        assert_eq!(store.info().unwrap(), None);

        let files = tempfile::tempdir().unwrap();
        let plain = files.path().join("ps2.dat");
        fs::write(&plain, DAT).unwrap();
        let info = store.import(&plain).unwrap();
        assert_eq!(info.games, 1);
        assert!(info.serials);
        assert_eq!(info.version, "2026-10-23 01-56-22");
        assert_eq!(info.file_name.as_deref(), Some("ps2.dat"));
        assert_eq!(info.checked_at, None);
        assert_eq!(store.info().unwrap(), Some(info));
        assert_eq!(fs::read_to_string(store.dat_path()).unwrap(), DAT);

        let zip = files.path().join("redump.zip");
        fs::write(
            &zip,
            zipped(
                "Sony - PlayStation 2 - Datfile (1) (2026-10-23).dat",
                DAT.as_bytes(),
            ),
        )
        .unwrap();
        let info = store.import(&zip).unwrap();
        assert_eq!(
            info.file_name.as_deref(),
            Some("Sony - PlayStation 2 - Datfile (1) (2026-10-23).dat")
        );

        // Garbage is refused and the stored DAT stays.
        let junk = files.path().join("junk.dat");
        fs::write(&junk, "<html/>").unwrap();
        assert!(store.import(&junk).is_err());
        let empty = files.path().join("empty.zip");
        fs::write(&empty, zipped("readme.txt", b"hi")).unwrap();
        assert!(store.import(&empty).is_err());
        assert_eq!(store.info().unwrap().unwrap().games, 1);
    }

    #[tokio::test]
    async fn asks_redump_at_most_once_a_week() {
        let data = tempfile::tempdir().unwrap();
        let store = RedumpStore::new(data.path());
        let files = tempfile::tempdir().unwrap();
        let plain = files.path().join("ps2.dat");
        fs::write(&plain, DAT).unwrap();
        store.import(&plain).unwrap();
        // Pretend Redump was asked an hour ago: no request goes out (the
        // client would fail on the real URL in a sandbox anyway).
        let mut info = store.info().unwrap().unwrap();
        info.checked_at = Some(now_secs() - 3600);
        store.write_info(&info).unwrap();
        let http = Http::new().unwrap();
        let update = store.update(&http).await.unwrap();
        assert!(matches!(update, Update::TooSoon(_)), "{update:?}");
        assert!(update.info().next_check() > now_secs());
    }
}
