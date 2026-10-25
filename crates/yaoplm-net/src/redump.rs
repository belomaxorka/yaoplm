//! The Redump DAT for dump verification (META-2, ADR-0007).
//!
//! `https://redump.info/datfile/ps2/serial,version` answers with a zip of
//! one Logiqx `.dat` that carries serials (research/03 §3.1). Checked on
//! 2026-10-23: about 1.3 MB zipped, 5.5 MB unpacked, no `ETag` and no
//! `Last-Modified`, so a conditional request saves nothing. The rule is
//! therefore our own: Redump is asked only when the user asks for an
//! update, and at most once a week whatever the answer was, a failed
//! download included. Without a network the user can import a DAT they
//! downloaded themselves (a `.dat` or the `.zip`), which is also how tests
//! and the e2e fixture get one.
//!
//! Layout under the app data folder (ARCHITECTURE.md §6):
//!
//! ```text
//! db/redump-ps2.dat    the Logiqx XML, unpacked
//! db/redump-ps2.json   where it came from and when ([`DatInfo`])
//! db/redump-ps2.asked  touched on every answer from Redump: the weekly rule
//! ```
//!
//! Every file is written to a temporary name, synced and renamed. A JSON
//! damaged anyway (the machine froze mid-write) is rebuilt from the DAT.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use yaoplm_hash::dat;

use crate::NetError;
use crate::cache::{now_secs, sha256_hex};
use crate::http::{Fetched, Http, Validators};

pub const DB_DIR: &str = "db";
pub const DAT_FILE: &str = "redump-ps2.dat";
pub const INFO_FILE: &str = "redump-ps2.json";
pub const ASKED_FILE: &str = "redump-ps2.asked";
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

/// How an update ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Update {
    /// Redump was asked less than a week ago: no request was made. There
    /// may be no DAT at all when that request failed.
    TooSoon {
        info: Option<DatInfo>,
        /// Seconds since 1970.
        next_check: u64,
    },
    /// Redump said nothing changed (304).
    NotModified(DatInfo),
    /// A new DAT is stored.
    Stored(DatInfo),
}

/// The stored DAT and its metadata.
#[derive(Debug, Clone)]
pub struct RedumpStore {
    dir: PathBuf,
    url: String,
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
        // One byte over the limit tells a bigger member from one that fits.
        (&mut entry)
            .take(MAX_DAT + 1)
            .read_to_end(&mut dat)
            .map_err(|e| NetError::Dat(format!("cannot unpack the DAT: {e}")))?;
        if dat.len() as u64 > MAX_DAT {
            return Err(NetError::Dat(format!("{entry_name} is too large")));
        }
        return Ok((dat, name));
    }
    Err(NetError::Dat("the zip holds no .dat file".into()))
}

fn modified_secs(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

impl RedumpStore {
    pub fn new(data_dir: &Path) -> Self {
        Self::with_url(data_dir, URL)
    }

    /// A store that asks `url` instead of Redump (tests).
    pub fn with_url(data_dir: &Path, url: &str) -> Self {
        Self {
            dir: data_dir.join(DB_DIR),
            url: url.to_owned(),
        }
    }

    pub fn dat_path(&self) -> PathBuf {
        self.dir.join(DAT_FILE)
    }

    fn info_path(&self) -> PathBuf {
        self.dir.join(INFO_FILE)
    }

    fn asked_path(&self) -> PathBuf {
        self.dir.join(ASKED_FILE)
    }

    /// The stored DAT's metadata, `None` before the first download or
    /// import, or when the DAT itself is gone or unreadable. Damaged
    /// metadata is rebuilt from the DAT.
    pub fn info(&self) -> Result<Option<DatInfo>, NetError> {
        if !self.dat_path().is_file() {
            return Ok(None);
        }
        let path = self.info_path();
        match fs::read(&path) {
            Ok(bytes) => {
                if let Ok(info) = serde_json::from_slice(&bytes) {
                    return Ok(Some(info));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(NetError::io(&path)(err)),
        }
        self.rebuild_info()
    }

    /// Metadata from the DAT itself, written back for next time.
    fn rebuild_info(&self) -> Result<Option<DatInfo>, NetError> {
        let path = self.dat_path();
        let dat = fs::read(&path).map_err(NetError::io(&path))?;
        let Ok(parsed) = dat::parse(&dat) else {
            return Ok(None);
        };
        if parsed.games.is_empty() {
            return Ok(None);
        }
        let info = DatInfo {
            source: path.display().to_string(),
            file_name: None,
            description: parsed.header.description.clone(),
            version: parsed.header.version.clone(),
            games: parsed.games.len() as u64,
            serials: parsed.games.iter().any(|g| g.serial.is_some()),
            sha256: sha256_hex(&dat),
            stored_at: modified_secs(&path).unwrap_or(0),
            checked_at: None,
            etag: None,
            last_modified: None,
        };
        // Best effort: the answer stands even if the folder is read-only.
        let _ = self.write_info(&info);
        Ok(Some(info))
    }

    fn write_info(&self, info: &DatInfo) -> Result<(), NetError> {
        let json = serde_json::to_vec_pretty(info).map_err(|e| NetError::Dat(e.to_string()))?;
        write_atomic(&self.info_path(), &json)
    }

    /// Seconds since 1970 of the last answer from Redump.
    pub fn last_asked(&self) -> Option<u64> {
        let marked = modified_secs(&self.asked_path());
        let recorded = self.info().ok().flatten().and_then(|i| i.checked_at);
        marked.max(recorded)
    }

    /// When Redump may be asked again; `None` when it never was.
    pub fn next_check(&self) -> Option<u64> {
        self.last_asked()
            .map(|at| at.saturating_add(MIN_INTERVAL.as_secs()))
    }

    fn mark_asked(&self) -> Result<(), NetError> {
        fs::create_dir_all(&self.dir).map_err(NetError::io(&self.dir))?;
        write_atomic(&self.asked_path(), now_secs().to_string().as_bytes())
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
        let checked_at = self.info().ok().flatten().and_then(|i| i.checked_at);
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
    /// Callers run one update or import at a time.
    pub async fn update(&self, http: &Http) -> Result<Update, NetError> {
        let current = self.info().ok().flatten();
        let now = now_secs();
        if let Some(next_check) = self.next_check()
            && now < next_check
        {
            return Ok(Update::TooSoon {
                info: current,
                next_check,
            });
        }
        let validators = current
            .as_ref()
            .map(|i| Validators {
                etag: i.etag.clone(),
                last_modified: i.last_modified.clone(),
            })
            .unwrap_or_default();
        let fetched = http
            .get_large(LIMITER, &self.url, &validators, MAX_DAT)
            .await;
        // Any answer counts towards the weekly rule, an error page or a
        // file that is no DAT included; no answer at all (offline) does not.
        if matches!(
            &fetched,
            Ok(_) | Err(NetError::Status { .. } | NetError::TooLarge(_))
        ) {
            self.mark_asked()?;
        }
        match fetched? {
            Fetched::NotModified => {
                let Some(mut info) = current else {
                    return Err(NetError::Dat("304 without a stored DAT".into()));
                };
                info.checked_at = Some(now);
                self.write_info(&info)?;
                Ok(Update::NotModified(info))
            }
            Fetched::NotFound => Err(NetError::Status {
                url: self.url.clone(),
                status: 404,
            }),
            Fetched::Ok(body) => {
                let file_name = body.file_name.clone();
                let validators = Validators {
                    etag: body.etag,
                    last_modified: body.last_modified,
                };
                self.store(body.bytes, &self.url, file_name, Some(now), validators)
                    .map(Update::Stored)
            }
        }
    }
}

/// Writes `<path>.part`, syncs it and renames it over `path`: after a crash
/// `path` holds the old bytes or the new ones, never a mix.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), NetError> {
    // A name of its own: two jobs may store the same answer at once (two
    // reads of one release), and a shared `.part` would vanish under one.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}-{}.part",
        std::process::id(),
        NEXT.fetch_add(1, AtomicOrdering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    let mut file = fs::File::create(&tmp).map_err(NetError::io(&tmp))?;
    file.write_all(bytes).map_err(NetError::io(&tmp))?;
    file.sync_all().map_err(NetError::io(&tmp))?;
    drop(file);
    fs::rename(&tmp, path).map_err(NetError::io(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn writers_of_one_file_at_once_all_succeed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("release.json");
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        write_atomic(&path, format!("answer {n}").as_bytes()).unwrap();
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().unwrap();
        }
        assert!(fs::read_to_string(&path).unwrap().starts_with("answer "));
        let left: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["release.json"], "no temporary file is left");
    }

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    #[test]
    fn damaged_metadata_is_rebuilt() {
        let data = tempfile::tempdir().unwrap();
        let store = RedumpStore::new(data.path());
        let files = tempfile::tempdir().unwrap();
        let plain = files.path().join("ps2.dat");
        fs::write(&plain, DAT).unwrap();
        store.import(&plain).unwrap();
        // A freeze left the JSON empty: it comes back from the DAT.
        fs::write(store.info_path(), b"").unwrap();
        let info = store.info().unwrap().unwrap();
        assert_eq!(info.games, 1);
        assert_eq!(info.version, "2026-10-23 01-56-22");
        let written = fs::read(store.info_path()).unwrap();
        assert!(serde_json::from_slice::<DatInfo>(&written).is_ok());
        // An import works over damaged metadata too.
        fs::write(store.info_path(), b"{").unwrap();
        assert_eq!(store.import(&plain).unwrap().games, 1);
        // A damaged DAT counts as none.
        fs::write(store.dat_path(), b"<datafile><game name='x'>").unwrap();
        fs::remove_file(store.info_path()).unwrap();
        assert_eq!(store.info().unwrap(), None);
    }

    /// A server that answers every request with `status` and `body`, and
    /// counts the requests.
    async fn answer(status: &'static str, body: &'static [u8]) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/datfile", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                seen.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body).await;
            }
        });
        (url, count)
    }

    #[tokio::test]
    async fn asks_redump_at_most_once_a_week() {
        let data = tempfile::tempdir().unwrap();
        let (url, count) = answer("200 OK", DAT.as_bytes()).await;
        let store = RedumpStore::with_url(data.path(), &url);
        let http = Http::new().unwrap();
        let Update::Stored(info) = store.update(&http).await.unwrap() else {
            panic!("expected a download");
        };
        assert_eq!(info.games, 1);
        assert_eq!(info.source, url);
        let again = store.update(&http).await.unwrap();
        assert!(
            matches!(again, Update::TooSoon { info: Some(_), next_check } if next_check > now_secs()),
            "{again:?}"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_request_counts_too() {
        let data = tempfile::tempdir().unwrap();
        let (url, count) = answer("404 Not Found", b"").await;
        let store = RedumpStore::with_url(data.path(), &url);
        let http = Http::new().unwrap();
        assert!(store.update(&http).await.is_err());
        let again = store.update(&http).await.unwrap();
        assert!(
            matches!(again, Update::TooSoon { info: None, .. }),
            "{again:?}"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // No answer at all (nothing listens) does not count.
        let offline = tempfile::tempdir().unwrap();
        let store = RedumpStore::with_url(offline.path(), "http://127.0.0.1:9/datfile");
        assert!(store.update(&http).await.is_err());
        assert_eq!(store.next_check(), None);
    }
}
