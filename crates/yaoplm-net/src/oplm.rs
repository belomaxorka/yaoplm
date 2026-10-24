//! OPL Manager's art backup on archive.org, read picture by picture
//! (ADR-0034, research/14 §3).
//!
//! `OPLM_ART_2024_09` is the last of the "entire OPLM ART database
//! backups" OPL Manager's author published (the private art server itself
//! is never asked, CLAUDE.md rule 3). It is one 5.5 GB zip64 whose members
//! are stored without compression, under OPL's own IDs:
//! `PS2/<ID>/<ID>_<TYPE>.png`, with numbered screenshots and backgrounds
//! (`SCR_00` is SCR, `SCR_01` is SCR2, `BG_00` is BG, research/13 §3).
//!
//! - The central directory (8.6 MB, about 103 000 members) is read once
//!   with two range requests and kept next to the art cache. The item is
//!   final, so it is never asked for again unless a read shows that the
//!   file changed.
//! - Each picture is then one range request for its local header and
//!   data, checked against the CRC-32 the directory gives. Only members
//!   the directory lists are asked for: archive.org answers a missing
//!   member of `view_archive.php` with a 503 after nine seconds, not a 404.
//! - The directory also tells which pictures are byte-identical to the
//!   picture of a game of another market ([`Picture::shared_across_markets`]):
//!   about 2% of covers and 1% of icons, spines and back covers carry the
//!   wrong release (research/14 §3.3).
//! - archive.org is slow (2-18 s a request) and sometimes answers 500: one
//!   connection, a second between requests, retries with back-off, the
//!   item's own servers from its metadata when the download URL fails, and
//!   a pause of ten minutes after the index could not be read.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use yaoplm_formats::art::ArtKind;

use crate::NetError;
use crate::http::{ByteRange, Http, MAX_BODY};
use crate::region::{self, Family};
use crate::remote_zip::{self, Directory, LOCAL_LEN, Member, Tail};

/// The backup YAOPLM reads.
pub const ITEM: &str = "OPLM_ART_2024_09";
/// The requests to archive.org count against this limiter.
pub const LIMITER: &str = "oplm";
/// Where the index lives inside the art cache folder.
pub const INDEX_DIR: &str = "oplm-index";
/// A central directory larger than this is not the backup.
const MAX_DIRECTORY: u64 = 64 << 20;
/// A local header's extra field is read in the same request as long as
/// it is no longer than this.
const LOCAL_SLACK: u64 = 256;
const INDEX_TIMEOUT: Duration = Duration::from_secs(300);
/// How long the source rests after the index could not be read.
const PAUSE: Duration = Duration::from_secs(600);
/// Numbered pictures offered as variants beyond the first.
const MAX_VARIANTS: usize = 4;

/// Where the backup is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Archive {
    pub item: String,
    /// URLs of the zip, tried in order.
    pub urls: Vec<String>,
    /// The item's metadata, asked once when every URL failed: it names
    /// the servers that hold the item.
    pub metadata: Option<String>,
}

impl Archive {
    pub fn archive_org(item: &str) -> Self {
        Self {
            item: item.to_owned(),
            urls: vec![format!("https://archive.org/download/{item}/{item}.zip")],
            metadata: Some(format!("https://archive.org/metadata/{item}")),
        }
    }

    /// The address a person can open for one member: archive.org extracts
    /// it on the fly. Used for the log and the cache; reads go through
    /// range requests.
    pub fn member_url(&self, name: &str) -> String {
        let base = self.urls.first().map_or("", String::as_str);
        format!("{base}/{name}")
    }
}

impl Default for Archive {
    fn default() -> Self {
        Self::archive_org(ITEM)
    }
}

/// A picture in the backup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub member: Member,
    /// `SLUS_200.71`, as the folder spells it.
    pub id: String,
    /// `COV`, `SCR_00`, `BG_01`, upper case.
    pub suffix: String,
    /// The same bytes (CRC-32 and size) are filed under a game of another
    /// market: one of them shows the wrong release.
    pub shared_across_markets: bool,
}

impl Picture {
    /// The cache key: `SLUS_200.71_COV`.
    pub fn key(&self) -> String {
        format!("{}_{}", self.id.to_ascii_uppercase(), self.suffix)
    }
}

/// The pictures of the backup by game.
#[derive(Debug, Default)]
pub struct Index {
    games: HashMap<String, Vec<Picture>>,
    pub zip_size: u64,
}

/// `SCR_03` → ("SCR", 3).
fn numbered(suffix: &str) -> Option<(&str, u32)> {
    let (base, number) = suffix.split_once('_')?;
    (matches!(base, "SCR" | "BG")
        && number.len() == 2
        && number.bytes().all(|b| b.is_ascii_digit()))
    .then(|| (base, number.parse().unwrap_or(u32::MAX)))
}

fn known_suffix(suffix: &str) -> bool {
    matches!(suffix, "COV" | "COV2" | "ICO" | "LAB" | "LGO") || numbered(suffix).is_some()
}

/// `PS2/SLUS_200.71/SLUS_200.71_COV.png` → the ID, the suffix and the
/// extension. PS1 pictures and anything else are left out (ADR-0033).
fn parse_name(name: &str) -> Option<(&str, String, String)> {
    let mut parts = name.split('/');
    let (system, id, file) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || !system.eq_ignore_ascii_case("PS2") || id.is_empty() {
        return None;
    }
    let (stem, ext) = file.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "bmp") {
        return None;
    }
    let head = stem.get(..id.len())?;
    let rest = stem.get(id.len()..)?.strip_prefix('_')?;
    let suffix = rest.to_ascii_uppercase();
    (head.eq_ignore_ascii_case(id) && known_suffix(&suffix)).then_some((id, suffix, ext))
}

impl Index {
    pub fn from_members(members: Vec<Member>, zip_size: u64) -> Self {
        let mut games: HashMap<String, Vec<(Picture, String)>> = HashMap::new();
        for member in members {
            let Some((id, suffix, ext)) = parse_name(&member.name) else {
                continue;
            };
            let id = id.to_owned();
            games.entry(id.to_ascii_uppercase()).or_default().push((
                Picture {
                    member,
                    id,
                    suffix,
                    shared_across_markets: false,
                },
                ext,
            ));
        }
        // One picture per suffix: PNG first, which OPL 1.2 reads.
        let mut games: HashMap<String, Vec<Picture>> = games
            .into_iter()
            .map(|(id, mut pictures)| {
                pictures.sort_by_key(|(p, ext)| (p.suffix.clone(), ext != "png"));
                pictures.dedup_by(|(a, _), (b, _)| a.suffix == b.suffix);
                (id, pictures.into_iter().map(|(p, _)| p).collect())
            })
            .collect();
        mark_shared(&mut games);
        Self { games, zip_size }
    }

    /// Games with at least one picture.
    pub fn games(&self) -> usize {
        self.games.len()
    }

    /// Every picture of one game.
    pub fn all(&self, id: &str) -> &[Picture] {
        self.games
            .get(&id.to_ascii_uppercase())
            .map_or(&[], Vec::as_slice)
    }

    /// The pictures of `id` that fill a slot of `kind`, best first: the
    /// one OPL Manager's numbering assigns (ADR-0033), then, for a
    /// background, further backgrounds and screenshots OPL does not show
    /// elsewhere. With `variants`, also further numbered screenshots and
    /// backgrounds to pick from.
    pub fn pictures(&self, id: &str, kind: ArtKind, variants: bool) -> Vec<&Picture> {
        let pictures = self.all(id);
        let by = |suffix: &str| pictures.iter().find(|p| p.suffix == suffix);
        let numbered_of = |base: &str, from: u32| {
            let mut found: Vec<&Picture> = pictures
                .iter()
                .filter(|p| numbered(&p.suffix).is_some_and(|(b, n)| b == base && n >= from))
                .collect();
            found.sort_by(|a, b| a.suffix.cmp(&b.suffix));
            found
        };
        let mut out: Vec<&Picture> = match kind {
            ArtKind::Cov => by("COV").into_iter().collect(),
            ArtKind::Cov2 => by("COV2").into_iter().collect(),
            ArtKind::Ico => by("ICO").into_iter().collect(),
            ArtKind::Lab => by("LAB").into_iter().collect(),
            ArtKind::Lgo => by("LGO").into_iter().collect(),
            ArtKind::Scr | ArtKind::Scr2 => {
                let first = by(if kind == ArtKind::Scr {
                    "SCR_00"
                } else {
                    "SCR_01"
                });
                let mut out: Vec<&Picture> = first.into_iter().collect();
                if variants {
                    out.extend(numbered_of("SCR", 2));
                }
                out
            }
            ArtKind::Bg => {
                let mut out = numbered_of("BG", 0);
                out.extend(numbered_of("SCR", 2));
                out
            }
        };
        out.truncate(if variants { MAX_VARIANTS } else { usize::MAX });
        out
    }
}

/// Flags the packaging pictures whose bytes occur under games of more
/// than one market.
fn mark_shared(games: &mut HashMap<String, Vec<Picture>>) {
    let mut families: HashMap<(String, u32, u64), HashSet<Family>> = HashMap::new();
    let packaging = |p: &Picture| matches!(p.suffix.as_str(), "COV" | "COV2" | "ICO" | "LAB");
    for pictures in games.values() {
        for p in pictures.iter().filter(|p| packaging(p)) {
            if let Some(family) = region::of_id(&p.id) {
                families
                    .entry((p.suffix.clone(), p.member.crc32, p.member.size))
                    .or_default()
                    .insert(family);
            }
        }
    }
    for pictures in games.values_mut() {
        for p in pictures.iter_mut().filter(|p| packaging(p)) {
            p.shared_across_markets = families
                .get(&(p.suffix.clone(), p.member.crc32, p.member.size))
                .is_some_and(|f| f.len() > 1);
        }
    }
}

/// What is kept next to the directory bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    item: String,
    zip_size: u64,
    etag: Option<String>,
    directory_offset: u64,
    directory_size: u64,
    entries: u64,
    fetched_at: u64,
    /// The URL that answered, asked first by the next process.
    #[serde(default)]
    url: Option<String>,
}

struct State {
    index: Option<Arc<Index>>,
    /// The URLs in the order to try; the last one that worked comes first.
    urls: Vec<String>,
    metadata_asked: bool,
    paused_until: Option<(Instant, String)>,
}

/// The backup as a source: its index, loaded once, and range reads.
pub struct Oplm {
    archive: Archive,
    /// `<data>/art-cache/oplm-index/<item>`.
    dir: PathBuf,
    state: tokio::sync::Mutex<State>,
}

impl std::fmt::Debug for Oplm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Oplm")
            .field("archive", &self.archive)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

fn now_secs() -> u64 {
    crate::cache::now_secs()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), NetError> {
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(NetError::io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(NetError::io(path))
}

impl Oplm {
    /// `art_dir` is the art cache folder: clearing the cache clears the
    /// index too.
    pub fn new(archive: Archive, art_dir: &Path) -> Self {
        let dir = art_dir
            .join(INDEX_DIR)
            .join(crate::cache::safe_key(&archive.item));
        let urls = archive.urls.clone();
        Self {
            archive,
            dir,
            state: tokio::sync::Mutex::new(State {
                index: None,
                urls,
                metadata_asked: false,
                paused_until: None,
            }),
        }
    }

    pub fn archive(&self) -> &Archive {
        &self.archive
    }

    /// The index, from memory, from disk or from archive.org.
    pub async fn index(&self, http: &Http) -> Result<Arc<Index>, NetError> {
        let mut state = self.state.lock().await;
        if let Some(index) = &state.index {
            return Ok(Arc::clone(index));
        }
        if let Some((until, why)) = &state.paused_until {
            if Instant::now() < *until {
                return Err(NetError::Http(format!(
                    "{}: not asked again for a while after: {why}",
                    self.archive.item
                )));
            }
            state.paused_until = None;
        }
        if let Some((index, url)) = self.load() {
            if let Some(url) = url {
                state.urls.retain(|u| *u != url);
                state.urls.insert(0, url);
            }
            let index = Arc::new(index);
            state.index = Some(Arc::clone(&index));
            return Ok(index);
        }
        match self.fetch_index(http, &mut state).await {
            Ok(index) => {
                let index = Arc::new(index);
                state.index = Some(Arc::clone(&index));
                Ok(index)
            }
            Err(err) => {
                state.paused_until = Some((Instant::now() + PAUSE, err.to_string()));
                Err(err)
            }
        }
    }

    /// The index saved by an earlier run, if it is whole, and the URL
    /// that answered then.
    fn load(&self) -> Option<(Index, Option<String>)> {
        let meta: Meta =
            serde_json::from_slice(&std::fs::read(self.dir.join("meta.json")).ok()?).ok()?;
        let directory = std::fs::read(self.dir.join("directory.bin")).ok()?;
        if meta.item != self.archive.item || directory.len() as u64 != meta.directory_size {
            return None;
        }
        let members = remote_zip::parse_central_directory(&directory).ok()?;
        (members.len() as u64 == meta.entries)
            .then(|| (Index::from_members(members, meta.zip_size), meta.url))
    }

    /// Forgets the index: the file on archive.org is not the one it
    /// describes any more.
    async fn drop_index(&self) {
        self.state.lock().await.index = None;
        let _ = std::fs::remove_file(self.dir.join("meta.json"));
        let _ = std::fs::remove_file(self.dir.join("directory.bin"));
    }

    async fn fetch_index(&self, http: &Http, state: &mut State) -> Result<Index, NetError> {
        let tail = self
            .range(
                http,
                state,
                ByteRange::Suffix(remote_zip::TAIL_LEN),
                INDEX_TIMEOUT,
            )
            .await?;
        let zip_size = tail.total;
        let directory: Directory = match remote_zip::parse_tail(&tail.bytes, tail.start)? {
            Tail::Directory(directory) => directory,
            Tail::Zip64RecordAt(at) => {
                let record = self
                    .range(
                        http,
                        state,
                        ByteRange::Span {
                            start: at,
                            len: remote_zip::ZIP64_EOCD_LEN as u64,
                        },
                        INDEX_TIMEOUT,
                    )
                    .await?;
                remote_zip::parse_zip64_record(&record.bytes)?
            }
        };
        if directory.size > MAX_DIRECTORY || directory.offset + directory.size > zip_size {
            return Err(NetError::Zip(format!(
                "{}: a central directory of {} bytes at {} does not fit",
                self.archive.item, directory.size, directory.offset
            )));
        }
        let bytes = self
            .range(
                http,
                state,
                ByteRange::Span {
                    start: directory.offset,
                    len: directory.size,
                },
                INDEX_TIMEOUT,
            )
            .await?;
        if bytes.total != zip_size || bytes.bytes.len() as u64 != directory.size {
            return Err(NetError::Zip(format!(
                "{}: the central directory came back cut short",
                self.archive.item
            )));
        }
        let members = remote_zip::parse_central_directory(&bytes.bytes)?;
        if members.len() as u64 != directory.entries {
            return Err(NetError::Zip(format!(
                "{}: {} members listed, {} expected",
                self.archive.item,
                members.len(),
                directory.entries
            )));
        }
        let meta = Meta {
            item: self.archive.item.clone(),
            zip_size,
            etag: bytes.etag.clone(),
            directory_offset: directory.offset,
            directory_size: directory.size,
            entries: directory.entries,
            fetched_at: now_secs(),
            url: state.urls.first().cloned(),
        };
        // The index works without its copy on disk; it is only asked for
        // again by the next process.
        if std::fs::create_dir_all(&self.dir).is_ok() {
            let saved =
                write_atomic(&self.dir.join("directory.bin"), &bytes.bytes).and_then(|()| {
                    let json = serde_json::to_vec_pretty(&meta)
                        .map_err(|e| NetError::Cache(e.to_string()))?;
                    write_atomic(&self.dir.join("meta.json"), &json)
                });
            if saved.is_err() {
                let _ = std::fs::remove_file(self.dir.join("meta.json"));
            }
        }
        Ok(Index::from_members(members, zip_size))
    }

    /// One range of the zip, from the first URL that answers. A URL that
    /// fails moves to the back; when all have failed, the item's servers
    /// are taken from its metadata once and tried too.
    async fn range(
        &self,
        http: &Http,
        state: &mut State,
        range: ByteRange,
        timeout: Duration,
    ) -> Result<crate::http::Ranged, NetError> {
        let mut errors = Vec::new();
        let mut tried = HashSet::new();
        loop {
            let Some(url) = state.urls.iter().find(|u| !tried.contains(*u)).cloned() else {
                if !state.metadata_asked
                    && let Some(metadata) = self.archive.metadata.clone()
                {
                    state.metadata_asked = true;
                    match self.servers(http, &metadata).await {
                        Ok(urls) => {
                            for url in urls {
                                if !state.urls.contains(&url) {
                                    state.urls.push(url);
                                }
                            }
                            continue;
                        }
                        Err(err) => errors.push(err.to_string()),
                    }
                }
                return Err(NetError::Http(errors.join("; ")));
            };
            tried.insert(url.clone());
            let max = match range {
                ByteRange::Span { len, .. } => len.max(MAX_BODY as u64),
                ByteRange::Suffix(len) => len.max(MAX_BODY as u64),
            };
            match http
                .get_range(LIMITER, &url, range, max, Some(timeout))
                .await
            {
                Ok(Some(ranged)) => {
                    // The URL that answered is asked first from now on.
                    state.urls.retain(|u| *u != url);
                    state.urls.insert(0, url);
                    return Ok(ranged);
                }
                Ok(None) => errors.push(format!("{url}: HTTP 404")),
                Err(err) => errors.push(err.to_string()),
            }
        }
    }

    /// `https://<d1><dir>/<item>.zip` and the like, from the item's
    /// metadata.
    async fn servers(&self, http: &Http, metadata: &str) -> Result<Vec<String>, NetError> {
        let body = match http.get(LIMITER, metadata, &Default::default()).await? {
            crate::http::Fetched::Ok(body) => body,
            _ => return Err(NetError::Http(format!("{metadata}: no metadata"))),
        };
        #[derive(Deserialize)]
        struct Item {
            server: Option<String>,
            d1: Option<String>,
            d2: Option<String>,
            dir: Option<String>,
        }
        let item: Item = serde_json::from_slice(&body.bytes)
            .map_err(|e| NetError::Http(format!("{metadata}: {e}")))?;
        let dir = item
            .dir
            .ok_or_else(|| NetError::Http(format!("{metadata}: no dir")))?;
        let name = &self.archive.item;
        // The same scheme as the metadata: https on archive.org.
        let scheme = metadata.split_once("://").map_or("https", |(s, _)| s);
        Ok([item.d1, item.d2, item.server]
            .into_iter()
            .flatten()
            .filter(|host| {
                !host.is_empty()
                    && host
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
            })
            .map(|host| format!("{scheme}://{host}{dir}/{name}.zip"))
            .collect())
    }

    /// The bytes of one picture, checked against the directory's CRC-32.
    pub async fn read(&self, http: &Http, picture: &Picture) -> Result<Vec<u8>, NetError> {
        let member = &picture.member;
        if member.size > MAX_BODY as u64 || member.compressed_size > MAX_BODY as u64 {
            return Err(NetError::TooLarge(picture.member.name.clone()));
        }
        let mut state = self.state.lock().await;
        let zip_size = state.index.as_ref().map_or(0, |i| i.zip_size);
        let wanted =
            LOCAL_LEN as u64 + member.name.len() as u64 + LOCAL_SLACK + member.compressed_size;
        let len = wanted
            .min(zip_size.saturating_sub(member.offset))
            .max(LOCAL_LEN as u64);
        let first = self
            .range(
                http,
                &mut state,
                ByteRange::Span {
                    start: member.offset,
                    len,
                },
                Duration::from_secs(60),
            )
            .await?;
        drop(state);
        if first.total != zip_size || first.start != member.offset {
            self.drop_index().await;
            return Err(NetError::Zip(format!(
                "{}: the file on archive.org changed; its index is read again next time",
                self.archive.item
            )));
        }
        let start = remote_zip::data_start(&first.bytes)?;
        let end = start + member.compressed_size;
        let mut bytes = first.bytes;
        if (bytes.len() as u64) < end {
            // A local extra field longer than the slack.
            let mut state = self.state.lock().await;
            let rest = self
                .range(
                    http,
                    &mut state,
                    ByteRange::Span {
                        start: member.offset + bytes.len() as u64,
                        len: end - bytes.len() as u64,
                    },
                    Duration::from_secs(60),
                )
                .await?;
            bytes.extend_from_slice(&rest.bytes);
        }
        let data = bytes
            .get(start as usize..end as usize)
            .ok_or_else(|| NetError::Zip(format!("{}: cut short", member.name)))?;
        let data = match member.method {
            0 => data.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(member.size as usize);
                flate2::read::DeflateDecoder::new(data)
                    .take(MAX_BODY as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| NetError::Zip(format!("{}: {e}", member.name)))?;
                out
            }
            other => {
                return Err(NetError::Zip(format!(
                    "{}: compression method {other} is not supported",
                    member.name
                )));
            }
        };
        if data.len() as u64 != member.size || crc32fast::hash(&data) != member.crc32 {
            self.drop_index().await;
            return Err(NetError::Zip(format!(
                "{}: the bytes do not match the index (CRC-32); it is read again next time",
                member.name
            )));
        }
        Ok(data)
    }
}

/// The mirror files that may hold `id`'s picture of `kind`, best first: a
/// folder with the backup's layout (an unpacked backup or a copy of it).
pub fn mirror_paths(id: &str, kind: ArtKind) -> Vec<(String, String)> {
    let suffixes: &[&str] = match kind {
        ArtKind::Cov => &["COV"],
        ArtKind::Cov2 => &["COV2"],
        ArtKind::Ico => &["ICO"],
        ArtKind::Lab => &["LAB"],
        ArtKind::Lgo => &["LGO"],
        ArtKind::Scr => &["SCR_00"],
        ArtKind::Scr2 => &["SCR_01"],
        ArtKind::Bg => &["BG_00"],
    };
    let id = id.to_ascii_uppercase();
    suffixes
        .iter()
        .flat_map(|suffix| {
            let key = format!("{id}_{suffix}");
            ["png", "jpg"].map(|ext| (format!("PS2/{id}/{id}_{suffix}.{ext}"), key.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, crc32: u32, size: u64) -> Member {
        Member {
            name: name.to_owned(),
            method: 0,
            crc32,
            compressed_size: size,
            size,
            offset: 0,
        }
    }

    #[test]
    fn names_and_slots() {
        let index = Index::from_members(
            vec![
                member("PS2/SLUS_200.71/SLUS_200.71_COV.png", 1, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_COV.jpg", 2, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_SCR_00.png", 3, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_SCR_01.png", 4, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_SCR_03.png", 5, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_SCR_02.png", 6, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_BG_00.png", 7, 10),
                member("PS2/slps_250.02/slps_250.02_lab.PNG", 8, 10),
                member("PS2/SLPS_250.02/SLPS_250.02_BG_01.png", 9, 10),
                member("PS1/SLUS_200.71/SLUS_200.71_ICO.png", 10, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_COV3.png", 11, 10),
                member("PS2/SLUS_200.71/SLUS_200.71_SCR_1.png", 12, 10),
                member("PS2/SLUS_200.71/notes.txt", 13, 10),
                member("PS2_LIST.csv", 14, 10),
                member("PS2/", 0, 0),
            ],
            1000,
        );
        assert_eq!(index.games(), 2);
        let suffixes = |kind, variants| -> Vec<String> {
            index
                .pictures("slus_200.71", kind, variants)
                .iter()
                .map(|p| p.suffix.clone())
                .collect()
        };
        // PNG wins over JPEG; PS1, unknown types and other files are out.
        let cov = index.pictures("SLUS_200.71", ArtKind::Cov, false);
        assert_eq!(cov.len(), 1);
        assert!(cov[0].member.name.ends_with(".png"));
        assert_eq!(cov[0].key(), "SLUS_200.71_COV");
        assert!(suffixes(ArtKind::Ico, true).is_empty());
        assert_eq!(suffixes(ArtKind::Scr, false), ["SCR_00"]);
        assert_eq!(suffixes(ArtKind::Scr, true), ["SCR_00", "SCR_02", "SCR_03"]);
        assert_eq!(suffixes(ArtKind::Scr2, false), ["SCR_01"]);
        // A background, then screenshots OPL shows nowhere else.
        assert_eq!(suffixes(ArtKind::Bg, false), ["BG_00", "SCR_02", "SCR_03"]);
        assert_eq!(suffixes(ArtKind::Bg, true), ["BG_00", "SCR_02", "SCR_03"]);
        // Case does not matter.
        let lab = index.pictures("SLPS_250.02", ArtKind::Lab, false);
        assert_eq!(lab[0].key(), "SLPS_250.02_LAB");
        assert_eq!(
            index.pictures("SLPS_250.02", ArtKind::Bg, false)[0].suffix,
            "BG_01"
        );
        assert!(
            index
                .pictures("SLES_000.00", ArtKind::Cov, false)
                .is_empty()
        );
    }

    #[test]
    fn pictures_shared_across_markets_are_flagged() {
        // Herdy Gerdy: the JP and KR IDs carry the US cover; the PAL
        // language variants share theirs, which is normal.
        let index = Index::from_members(
            vec![
                member("PS2/SLUS_202.31/SLUS_202.31_COV.png", 0xAA, 100),
                member("PS2/SLPS_251.94/SLPS_251.94_COV.png", 0xAA, 100),
                member("PS2/SLKA_250.03/SLKA_250.03_COV.png", 0xAA, 100),
                member("PS2/SLES_533.32/SLES_533.32_COV.png", 0xBB, 100),
                member("PS2/SLES_533.33/SLES_533.33_COV.png", 0xBB, 100),
                // Same bytes, other slot: not a match.
                member("PS2/SLES_533.32/SLES_533.32_ICO.png", 0xAA, 100),
                // Screenshots are shared across markets as they should be.
                member("PS2/SLUS_202.31/SLUS_202.31_SCR_00.png", 0xCC, 100),
                member("PS2/SLES_533.32/SLES_533.32_SCR_00.png", 0xCC, 100),
                // A homebrew ID says nothing about its market.
                member("PS2/DOOM_123.45/DOOM_123.45_COV.png", 0xBB, 100),
            ],
            1000,
        );
        let shared = |id: &str, kind| index.pictures(id, kind, false)[0].shared_across_markets;
        assert!(shared("SLUS_202.31", ArtKind::Cov));
        assert!(shared("SLPS_251.94", ArtKind::Cov));
        assert!(shared("SLKA_250.03", ArtKind::Cov));
        assert!(!shared("SLES_533.32", ArtKind::Cov));
        assert!(!shared("SLES_533.32", ArtKind::Ico));
        assert!(!shared("SLES_533.32", ArtKind::Scr));
        assert!(!shared("DOOM_123.45", ArtKind::Cov));
    }

    #[test]
    fn mirror_layout() {
        let paths = mirror_paths("slus_200.71", ArtKind::Scr2);
        assert_eq!(
            paths,
            [
                (
                    "PS2/SLUS_200.71/SLUS_200.71_SCR_01.png".to_owned(),
                    "SLUS_200.71_SCR_01".to_owned()
                ),
                (
                    "PS2/SLUS_200.71/SLUS_200.71_SCR_01.jpg".to_owned(),
                    "SLUS_200.71_SCR_01".to_owned()
                ),
            ]
        );
        assert_eq!(
            Archive::default().member_url("PS2/A/A_COV.png"),
            "https://archive.org/download/OPLM_ART_2024_09/OPLM_ART_2024_09.zip/PS2/A/A_COV.png"
        );
    }
}
