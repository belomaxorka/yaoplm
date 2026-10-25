//! Loader releases from GitHub and the store of loader ELFs (SPEC LDR-1,
//! LDR-2, ADR-0014, ADR-0042).
//!
//! The network is used only on a user action, and only for:
//! - a release's description, `GET /repos/<repo>/releases/tags/<tag>`,
//!   asked with the `ETag` of the last answer (a 304 costs nothing against
//!   GitHub's 60 requests an hour, research/07 §5);
//! - the list of a repository's releases, `GET /repos/<repo>/releases`, the
//!   same way, for "Another version…" (LDR-6);
//! - files of that release, only from
//!   `https://github.com/<repo>/releases/download/` (GitHub redirects to
//!   its CDN);
//! - the catalogue from `main` on raw.githubusercontent.com.
//!
//! A mirror folder stands in for all three (tests, CI, offline use):
//!
//! ```text
//! <mirror>/repos/<owner>/<repo>/releases/tags/<tag>.json
//! <mirror>/repos/<owner>/<repo>/releases.json
//! <mirror>/<owner>/<repo>/releases/download/<tag>/<asset>
//! <mirror>/data/loaders/<file>
//! ```
//!
//! The store under the app data folder:
//!
//! ```text
//! loaders/releases/<owner>__<repo>/<tag>.json   the last answer, with .etag;
//!                                               its time is the last check (LDR-4)
//! loaders/releases/<owner>__<repo>.list.json    the last list of releases, with .etag
//! loaders/catalog/<file>                        the catalogue from main, once it read
//! loaders/elf/<sha256>.elf, <sha256>.json       ELFs ready to install and what they are
//! ```
//!
//! Every file is written to a temporary name, synced and renamed.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::NetError;
use crate::cache::now_secs;
use crate::http::{Fetched, Http, Validators};
use crate::redump::write_atomic;

pub const DIR: &str = "loaders";
pub const API: &str = "https://api.github.com";
pub const DOWNLOAD: &str = "https://github.com/";
/// The catalogue of YAOPLM's `main` branch.
pub const RAW_MAIN: &str = "https://raw.githubusercontent.com/belomaxorka/yaoplm/main/";
const LIMIT_API: &str = "github-api";
const LIMIT_DOWNLOAD: &str = "github-download";
const LIMIT_RAW: &str = "github-raw";
/// A release description is a few kilobytes; upstream's 1.1.0 is 20 KB.
const MAX_JSON: u64 = 4 << 20;
/// Checksum files and catalogue files are small.
pub const MAX_TEXT: u64 = 1 << 20;

/// Where releases and the catalogue come from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseSource {
    /// GitHub, at these bases (tests point them at a local server).
    Remote {
        api: String,
        download: String,
        raw: String,
    },
    Mirror(PathBuf),
}

impl ReleaseSource {
    pub fn github() -> Self {
        Self::Remote {
            api: API.to_owned(),
            download: DOWNLOAD.to_owned(),
            raw: RAW_MAIN.to_owned(),
        }
    }
}

/// `owner/name` with plain components only.
fn repo_parts(repo: &str) -> Result<(&str, &str), NetError> {
    let bad = || NetError::Http(format!("{repo}: not an owner/name repository"));
    let (owner, name) = repo.split_once('/').ok_or_else(bad)?;
    let plain = |s: &str| {
        !s.is_empty()
            && s != "."
            && s != ".."
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if plain(owner) && plain(name) {
        Ok((owner, name))
    } else {
        Err(bad())
    }
}

/// A path below `base` from `/`-separated plain parts, or `None`.
fn below(base: &Path, rest: &str) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for part in rest.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(['\\', ':']) {
            return None;
        }
        path.push(part);
    }
    Some(path)
}

/// Tags are URL path segments: refuse anything that is not one.
fn plain_tag(tag: &str) -> Result<&str, NetError> {
    if tag.is_empty() || tag.contains(['/', '\\', '?', '#', ' ']) || tag == "." || tag == ".." {
        return Err(NetError::Http(format!("{tag}: not a release tag")));
    }
    Ok(tag)
}

/// A release's description as last answered, and its `ETag`.
pub type CachedRelease = (Vec<u8>, Option<String>);

/// The catalogue last read from `main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredCatalog {
    /// By path under `data/`.
    pub files: BTreeMap<String, String>,
    /// Seconds since 1970.
    pub stored_at: u64,
    /// The YAOPLM that read it: a newer program carries a catalogue at
    /// least as new, so it ignores the copy.
    pub program: String,
}

/// A release file kept as it was downloaded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredFile {
    /// Its name in the release (`OPNPS2LD-LANGS.7z`).
    pub file: String,
    pub sha256: String,
}

/// What a download is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ElfRecord {
    pub sha256: String,
    pub size: u64,
    /// The version inside the ELF, when it can be read.
    pub version: Option<String>,
    /// The catalogue build.
    pub build: Option<String>,
    /// The release tag it came from; `None` for a user's own file.
    pub tag: Option<String>,
    /// The release file it came from, or the user's file name.
    pub file: String,
    /// The member of `file` when that is an archive.
    #[serde(default)]
    pub member: Option<String>,
    /// The SHA-256 of `file` as downloaded: the same release file again
    /// is not downloaded twice.
    #[serde(default)]
    pub file_sha256: Option<String>,
    /// The release page, or the path of the user's file.
    pub source: String,
    /// The release's language pack, when it was downloaded too.
    #[serde(default)]
    pub languages: Option<StoredFile>,
    /// A release or the catalogue confirmed its hash before download.
    pub confirmed: bool,
    /// Seconds since 1970.
    pub stored_at: u64,
}

/// The loader folder of the app data directory.
#[derive(Debug, Clone)]
pub struct LoaderStore {
    dir: PathBuf,
}

impl LoaderStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            dir: data_dir.join(DIR),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn release_path(&self, repo: &str, tag: &str) -> Result<PathBuf, NetError> {
        let (owner, name) = repo_parts(repo)?;
        Ok(self
            .dir
            .join("releases")
            .join(format!("{owner}__{name}"))
            .join(format!("{}.json", plain_tag(tag)?)))
    }

    fn list_path(&self, repo: &str) -> Result<PathBuf, NetError> {
        let (owner, name) = repo_parts(repo)?;
        Ok(self
            .dir
            .join("releases")
            .join(format!("{owner}__{name}.list.json")))
    }

    /// The last answer for a release and its `ETag`.
    pub fn cached_release(&self, repo: &str, tag: &str) -> Result<Option<CachedRelease>, NetError> {
        Ok(cached_at(&self.release_path(repo, tag)?))
    }

    /// The last list of a repository's releases and its `ETag`.
    pub fn cached_list(&self, repo: &str) -> Result<Option<CachedRelease>, NetError> {
        Ok(cached_at(&self.list_path(repo)?))
    }

    /// When the release was last read, Unix seconds: the device card says
    /// how fresh its "Update to …" is without asking GitHub (LDR-4).
    pub fn release_checked_at(&self, repo: &str, tag: &str) -> Option<u64> {
        let modified = fs::metadata(self.release_path(repo, tag).ok()?)
            .ok()?
            .modified()
            .ok()?;
        modified
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    }

    fn put_release(
        &self,
        repo: &str,
        tag: &str,
        json: &[u8],
        etag: Option<&str>,
    ) -> Result<(), NetError> {
        put_at(&self.release_path(repo, tag)?, json, etag)
    }

    fn put_list(&self, repo: &str, json: &[u8], etag: Option<&str>) -> Result<(), NetError> {
        put_at(&self.list_path(repo)?, json, etag)
    }

    fn catalog_dir(&self) -> PathBuf {
        self.dir.join("catalog")
    }

    /// The catalogue from `main` by path under `data/`, if one was stored.
    pub fn catalog(&self) -> Option<StoredCatalog> {
        let dir = self.catalog_dir().join("loaders");
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(&dir).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".toml") {
                files.insert(
                    format!("loaders/{name}"),
                    fs::read_to_string(entry.path()).ok()?,
                );
            }
        }
        let stored_at = fs::read_to_string(self.catalog_dir().join("stored-at"))
            .ok()
            .and_then(|s| s.trim().parse().ok())?;
        let program = fs::read_to_string(self.catalog_dir().join("program"))
            .ok()?
            .trim()
            .to_owned();
        files
            .contains_key("loaders/index.toml")
            .then_some(StoredCatalog {
                files,
                stored_at,
                program,
            })
    }

    /// Replaces the stored catalogue with `files` (paths under `data/`);
    /// `program` is the version of YAOPLM that read it.
    pub fn put_catalog(
        &self,
        files: &BTreeMap<String, String>,
        program: &str,
    ) -> Result<u64, NetError> {
        let next = self.dir.join("catalog.new");
        let _ = fs::remove_dir_all(&next);
        for (path, text) in files {
            let target = below(&next, path)
                .ok_or_else(|| NetError::Http(format!("{path}: not a catalogue file")))?;
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir).map_err(NetError::io(dir))?;
            }
            write_atomic(&target, text.as_bytes())?;
        }
        let stored = now_secs();
        write_atomic(&next.join("stored-at"), stored.to_string().as_bytes())?;
        write_atomic(&next.join("program"), program.as_bytes())?;
        let current = self.catalog_dir();
        let old = self.dir.join("catalog.old");
        let _ = fs::remove_dir_all(&old);
        if current.exists() {
            fs::rename(&current, &old).map_err(NetError::io(&current))?;
        }
        fs::rename(&next, &current).map_err(NetError::io(&next))?;
        let _ = fs::remove_dir_all(&old);
        Ok(stored)
    }

    fn elf_path(&self, sha256: &str, extension: &str) -> Result<PathBuf, NetError> {
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(NetError::Http(format!("{sha256}: not a SHA-256")));
        }
        Ok(self.dir.join("elf").join(format!("{sha256}.{extension}")))
    }

    /// Stores an ELF and what it is; the same ELF again keeps one copy.
    pub fn put_elf(&self, record: &ElfRecord, bytes: &[u8]) -> Result<(), NetError> {
        let path = self.elf_path(&record.sha256, "elf")?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(NetError::io(dir))?;
        }
        write_atomic(&path, bytes)?;
        let json = serde_json::to_vec_pretty(record).map_err(|e| NetError::Cache(e.to_string()))?;
        write_atomic(&self.elf_path(&record.sha256, "json")?, &json)
    }

    /// The stored ELF with this SHA-256.
    pub fn elf(&self, sha256: &str) -> Result<Option<Vec<u8>>, NetError> {
        let path = self.elf_path(sha256, "elf")?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(NetError::io(&path)(e)),
        }
    }

    fn languages_path(&self, sha256: &str) -> Result<PathBuf, NetError> {
        // The same check as for ELFs: a SHA-256, nothing else.
        self.elf_path(sha256, "elf")?;
        Ok(self.dir.join("langs").join(sha256))
    }

    /// Keeps a language pack by its SHA-256.
    pub fn put_languages(&self, sha256: &str, bytes: &[u8]) -> Result<(), NetError> {
        let path = self.languages_path(sha256)?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(NetError::io(dir))?;
        }
        write_atomic(&path, bytes)
    }

    /// The language pack with this SHA-256, if kept.
    pub fn languages(&self, sha256: &str) -> Result<Option<Vec<u8>>, NetError> {
        let path = self.languages_path(sha256)?;
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(NetError::io(&path)(e)),
        }
    }

    /// Rewrites what a stored ELF is (its language pack came later).
    pub fn put_record(&self, record: &ElfRecord) -> Result<(), NetError> {
        let json = serde_json::to_vec_pretty(record).map_err(|e| NetError::Cache(e.to_string()))?;
        write_atomic(&self.elf_path(&record.sha256, "json")?, &json)
    }

    /// The record of the stored ELF with this SHA-256.
    pub fn record(&self, sha256: &str) -> Option<ElfRecord> {
        self.elfs().into_iter().find(|r| r.sha256 == sha256)
    }

    /// The file the stored ELF with this SHA-256 is in.
    pub fn elf_file(&self, sha256: &str) -> Result<PathBuf, NetError> {
        self.elf_path(sha256, "elf")
    }

    /// Every stored ELF, newest first. A record without its ELF is skipped.
    pub fn elfs(&self) -> Vec<ElfRecord> {
        let Ok(entries) = fs::read_dir(self.dir.join("elf")) else {
            return Vec::new();
        };
        let mut records: Vec<ElfRecord> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .filter_map(|e| serde_json::from_slice::<ElfRecord>(&fs::read(e.path()).ok()?).ok())
            .filter(|r| self.elf_path(&r.sha256, "elf").is_ok_and(|p| p.is_file()))
            .collect();
        records.sort_by(|a, b| b.stored_at.cmp(&a.stored_at).then(a.sha256.cmp(&b.sha256)));
        records
    }
}

/// A stored answer and its `ETag`.
fn cached_at(path: &Path) -> Option<CachedRelease> {
    let json = fs::read(path).ok()?;
    let etag = fs::read_to_string(path.with_extension("etag")).ok();
    Some((json, etag))
}

/// Stores an answer and its `ETag` (none: the old one goes).
fn put_at(path: &Path, json: &[u8], etag: Option<&str>) -> Result<(), NetError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(NetError::io(dir))?;
    }
    write_atomic(path, json)?;
    let etag_path = path.with_extension("etag");
    match etag {
        Some(etag) => write_atomic(&etag_path, etag.as_bytes()),
        None => match fs::remove_file(&etag_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(NetError::io(&etag_path)(e)),
        },
    }
}

/// A release's description and where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseJson {
    pub json: Vec<u8>,
    /// The cache answered: GitHub said nothing changed (304), or GitHub
    /// could not be reached and an earlier answer was used.
    pub cached: bool,
}

/// GitHub, or a mirror of it, through one HTTP client and the store.
pub struct LoaderNet<'a> {
    pub http: &'a Http,
    pub source: &'a ReleaseSource,
    pub store: &'a LoaderStore,
}

impl LoaderNet<'_> {
    /// The description of release `tag` of `repo`.
    pub async fn release(&self, repo: &str, tag: &str) -> Result<ReleaseJson, NetError> {
        let (owner, name) = repo_parts(repo)?;
        let tag = plain_tag(tag)?;
        match self.source {
            ReleaseSource::Mirror(dir) => {
                let path = below(
                    dir,
                    &format!("repos/{owner}/{name}/releases/tags/{tag}.json"),
                )
                .ok_or_else(|| NetError::Http(format!("{tag}: not a release tag")))?;
                let json = fs::read(&path).map_err(NetError::io(&path))?;
                // The mirror stands in for GitHub: the card reads its
                // answer offline like GitHub's.
                self.store.put_release(repo, tag, &json, None)?;
                Ok(ReleaseJson {
                    json,
                    cached: false,
                })
            }
            ReleaseSource::Remote { api, .. } => {
                let url = format!(
                    "{}/repos/{owner}/{name}/releases/tags/{tag}",
                    api.trim_end_matches('/')
                );
                let cached = self.store.cached_release(repo, tag)?;
                let validators = Validators {
                    etag: cached.as_ref().and_then(|(_, etag)| etag.clone()),
                    last_modified: None,
                };
                match self
                    .http
                    .get_large(LIMIT_API, &url, &validators, MAX_JSON)
                    .await?
                {
                    Fetched::Ok(body) => {
                        self.store
                            .put_release(repo, tag, &body.bytes, body.etag.as_deref())?;
                        Ok(ReleaseJson {
                            json: body.bytes,
                            cached: false,
                        })
                    }
                    Fetched::NotModified => match cached {
                        Some((json, etag)) => {
                            // Checked just now: the stored answer is fresh again.
                            self.store.put_release(repo, tag, &json, etag.as_deref())?;
                            Ok(ReleaseJson { json, cached: true })
                        }
                        None => Err(NetError::Http(format!(
                            "{url}: 304 without a cached answer"
                        ))),
                    },
                    Fetched::NotFound => Err(NetError::Status { url, status: 404 }),
                }
            }
        }
    }

    /// Every release of `repo`, newest first (LDR-6): GitHub's first page of
    /// a hundred, which holds every release the catalogue offers.
    pub async fn releases(&self, repo: &str) -> Result<ReleaseJson, NetError> {
        let (owner, name) = repo_parts(repo)?;
        match self.source {
            ReleaseSource::Mirror(dir) => {
                let path = below(dir, &format!("repos/{owner}/{name}/releases.json"))
                    .ok_or_else(|| NetError::Http(format!("{repo}: not a repository")))?;
                let json = fs::read(&path).map_err(NetError::io(&path))?;
                self.store.put_list(repo, &json, None)?;
                Ok(ReleaseJson {
                    json,
                    cached: false,
                })
            }
            ReleaseSource::Remote { api, .. } => {
                let url = format!(
                    "{}/repos/{owner}/{name}/releases?per_page=100",
                    api.trim_end_matches('/')
                );
                let cached = self.store.cached_list(repo)?;
                let validators = Validators {
                    etag: cached.as_ref().and_then(|(_, etag)| etag.clone()),
                    last_modified: None,
                };
                match self
                    .http
                    .get_large(LIMIT_API, &url, &validators, MAX_JSON)
                    .await?
                {
                    Fetched::Ok(body) => {
                        self.store
                            .put_list(repo, &body.bytes, body.etag.as_deref())?;
                        Ok(ReleaseJson {
                            json: body.bytes,
                            cached: false,
                        })
                    }
                    Fetched::NotModified => match cached {
                        Some((json, _)) => Ok(ReleaseJson { json, cached: true }),
                        None => Err(NetError::Http(format!(
                            "{url}: 304 without a cached answer"
                        ))),
                    },
                    Fetched::NotFound => Err(NetError::Status { url, status: 404 }),
                }
            }
        }
    }

    /// A release file, at most `max` bytes. `url` must be a release
    /// download of `repo`.
    pub async fn file(&self, repo: &str, url: &str, max: u64) -> Result<Vec<u8>, NetError> {
        let (owner, name) = repo_parts(repo)?;
        let prefix = format!("{DOWNLOAD}{owner}/{name}/releases/download/");
        let refused = || NetError::Http(format!("{url}: not a release download of {repo}"));
        let rest = url
            .get(..prefix.len())
            .filter(|p| p.eq_ignore_ascii_case(&prefix))
            .map(|_| &url[DOWNLOAD.len()..])
            .ok_or_else(refused)?;
        match self.source {
            ReleaseSource::Mirror(dir) => {
                let path = below(dir, rest).ok_or_else(refused)?;
                let len = fs::metadata(&path).map_err(NetError::io(&path))?.len();
                if len > max {
                    return Err(NetError::TooLarge(url.to_owned()));
                }
                fs::read(&path).map_err(NetError::io(&path))
            }
            ReleaseSource::Remote { download, .. } => {
                let url = format!("{download}{rest}");
                match self
                    .http
                    .get_large(LIMIT_DOWNLOAD, &url, &Validators::default(), max)
                    .await?
                {
                    Fetched::Ok(body) => Ok(body.bytes),
                    Fetched::NotModified | Fetched::NotFound => {
                        Err(NetError::Status { url, status: 404 })
                    }
                }
            }
        }
    }

    /// A file of the catalogue on `main` (`loaders/index.toml`); `None`
    /// when `main` has no such file.
    pub async fn catalog_file(&self, path: &str) -> Result<Option<String>, NetError> {
        let bad = || NetError::Http(format!("{path}: not a catalogue file"));
        if !path.starts_with("loaders/") {
            return Err(bad());
        }
        let bytes = match self.source {
            ReleaseSource::Mirror(dir) => {
                let file = below(dir, &format!("data/{path}")).ok_or_else(bad)?;
                match fs::read(&file) {
                    Ok(bytes) => bytes,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(NetError::io(&file)(e)),
                }
            }
            ReleaseSource::Remote { raw, .. } => {
                below(Path::new("x"), path).ok_or_else(bad)?;
                let url = format!("{raw}data/{path}");
                match self
                    .http
                    .get_large(LIMIT_RAW, &url, &Validators::default(), MAX_TEXT)
                    .await?
                {
                    Fetched::Ok(body) => body.bytes,
                    Fetched::NotFound => return Ok(None),
                    Fetched::NotModified => return Err(bad()),
                }
            }
        };
        String::from_utf8(bytes).map(Some).map_err(|_| bad())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::test_server::{Reply, serve};

    fn remote(base: &str) -> ReleaseSource {
        ReleaseSource::Remote {
            api: base.to_owned(),
            download: format!("{base}/"),
            raw: format!("{base}/raw/"),
        }
    }

    #[test]
    fn a_mirror_stands_in_for_github() {
        let dir = tempfile::tempdir().unwrap();
        let mirror = dir.path().join("mirror");
        let tags = mirror.join("repos/o/r/releases/tags");
        fs::create_dir_all(&tags).unwrap();
        fs::write(tags.join("latest.json"), b"{\"tag_name\":\"latest\"}").unwrap();
        let assets = mirror.join("o/r/releases/download/latest");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("OPNPS2LD.ELF"), b"elf").unwrap();
        fs::create_dir_all(mirror.join("data/loaders")).unwrap();
        fs::write(mirror.join("data/loaders/index.toml"), "schema = 1\n").unwrap();

        let http = Http::new().unwrap();
        let source = ReleaseSource::Mirror(mirror);
        let store = LoaderStore::new(&dir.path().join("data"));
        let net = LoaderNet {
            http: &http,
            source: &source,
            store: &store,
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            assert_eq!(
                net.release("o/r", "latest").await.unwrap().json,
                b"{\"tag_name\":\"latest\"}"
            );
            let url = "https://github.com/o/r/releases/download/latest/OPNPS2LD.ELF";
            assert_eq!(net.file("o/r", url, 10).await.unwrap(), b"elf");
            assert!(matches!(
                net.file("o/r", url, 2).await,
                Err(NetError::TooLarge(_))
            ));
            // Another repository's file, a file outside the mirror, another host.
            for url in [
                "https://github.com/x/y/releases/download/latest/OPNPS2LD.ELF",
                "https://github.com/o/r/releases/download/../../../../etc/passwd",
                "https://example.org/o/r/releases/download/latest/OPNPS2LD.ELF",
            ] {
                assert!(net.file("o/r", url, 10).await.is_err(), "{url}");
            }
            assert_eq!(
                net.catalog_file("loaders/index.toml")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("schema = 1\n")
            );
            assert_eq!(net.catalog_file("loaders/none.toml").await.unwrap(), None);
            assert!(net.catalog_file("../secret").await.is_err());
            assert!(net.release("o/r", "../x").await.is_err());
            assert!(net.release("o", "latest").await.is_err());
        });
    }

    #[test]
    fn github_is_asked_with_the_last_etag() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut routes = HashMap::new();
            let mut release = Reply::ok("application/json", b"{\"tag_name\":\"latest\"}".to_vec());
            release.headers.push(("ETag", "\"v1\"".to_owned()));
            routes.insert("/repos/o/r/releases/tags/latest".to_owned(), release);
            routes.insert(
                "/o/r/releases/download/latest/OPNPS2LD.ELF".to_owned(),
                Reply::ok("application/octet-stream", b"elf".to_vec()),
            );
            routes.insert(
                "/raw/data/loaders/index.toml".to_owned(),
                Reply::ok("text/plain", b"schema = 1\n".to_vec()),
            );
            let (base, seen) = serve(routes).await;
            let dir = tempfile::tempdir().unwrap();
            let http = Http::new().unwrap();
            let source = remote(&base);
            let store = LoaderStore::new(dir.path());
            let net = LoaderNet {
                http: &http,
                source: &source,
                store: &store,
            };

            let first = net.release("o/r", "latest").await.unwrap();
            assert!(!first.cached);
            let second = net.release("o/r", "latest").await.unwrap();
            assert!(second.cached, "the server answers 304 to the stored ETag");
            assert_eq!(second.json, first.json);
            {
                let seen = seen.lock().unwrap();
                let (_, headers) = &seen.requests[1];
                assert_eq!(
                    headers.get("if-none-match").map(String::as_str),
                    Some("\"v1\"")
                );
                assert!(
                    headers
                        .get("user-agent")
                        .is_some_and(|ua| ua.starts_with("YAOPLM/"))
                );
            }
            // A release download, with the real host replaced by the test one.
            let url = "https://github.com/o/r/releases/download/latest/OPNPS2LD.ELF";
            assert_eq!(net.file("o/r", url, 10).await.unwrap(), b"elf");
            assert_eq!(
                net.catalog_file("loaders/index.toml")
                    .await
                    .unwrap()
                    .as_deref(),
                Some("schema = 1\n")
            );
        });
    }

    #[test]
    fn the_store_keeps_elfs_and_one_catalogue() {
        let dir = tempfile::tempdir().unwrap();
        let store = LoaderStore::new(dir.path());
        assert!(store.elfs().is_empty());
        assert!(store.catalog().is_none());
        let sha = "a".repeat(64);
        let record = ElfRecord {
            sha256: sha.clone(),
            size: 3,
            version: Some("v1.1.0".into()),
            build: Some("opl-stable".into()),
            tag: Some("v1.1.0".into()),
            file: "OPNPS2LD-v1.1.0.7z".into(),
            member: Some("OPNPS2LD/OPNPS2LD-v1.1.0.ELF".into()),
            file_sha256: Some("c".repeat(64)),
            languages: None,
            source: "https://github.com/o/r/releases/tag/v1.1.0".into(),
            confirmed: true,
            stored_at: 5,
        };
        store.put_elf(&record, b"elf").unwrap();
        assert_eq!(store.elf(&sha).unwrap().as_deref(), Some(b"elf".as_slice()));
        assert_eq!(store.elfs(), [record]);
        assert!(store.elf("../x").is_err());
        assert_eq!(store.elf(&"b".repeat(64)).unwrap(), None);

        let mut files = BTreeMap::new();
        files.insert("loaders/index.toml".to_owned(), "schema = 1\n".to_owned());
        files.insert(
            "loaders/opl-beta.toml".to_owned(),
            "id = \"opl-beta\"\n".to_owned(),
        );
        store.put_catalog(&files, "0.1.0").unwrap();
        let got = store.catalog().unwrap();
        assert_eq!((got.files, got.program.as_str()), (files.clone(), "0.1.0"));
        // A second catalogue replaces the first one whole.
        files.remove("loaders/opl-beta.toml");
        store.put_catalog(&files, "0.1.0").unwrap();
        assert_eq!(store.catalog().unwrap().files, files);
        let mut bad = BTreeMap::new();
        bad.insert("../x".to_owned(), String::new());
        assert!(store.put_catalog(&bad, "0.1.0").is_err());
    }
}
