//! Dump verification over the API (GAME-5, META-2).
//!
//! `VerifyGames` is a job: it hashes every game in scope (the hash cache in
//! `cache.sqlite` answers for images that did not change), matches the
//! hashes against the stored Redump DAT and logs one line per game.
//! `UpdateRedump` downloads the DAT (at most once a week) or imports a
//! file the user has; `RedumpStatus` tells what is stored. Nothing here
//! writes to a device.
//!
//! Neither the DAT nor the cache is needed to hash: without a usable DAT
//! every verdict is "no DAT", without the cache every image is read. A
//! scan only reads an existing cache and never creates the app data folder.

use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError};

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_core::{Game as CoreGame, GameLocation, Phrase as Said, ScanOptions};
use yaoplm_hash::redump::{Candidate, Data, RedumpIndex, Unknown, Verdict};
use yaoplm_hash::{Algorithms, HashError, Hashes};
use yaoplm_net::cache::{Cache, HashedFile};
use yaoplm_net::redump::{DatInfo, RedumpStore, Update};

use crate::art::Net;
use crate::error::net_error;
use crate::jobs::{JobContext, JobLogEntry, JobResult, now_ms};
use crate::library::{GameFormat, LoaderProfile, Media};
use crate::paths::display_path;
use crate::phrase::Phrase;
use crate::{ApiError, ErrorCode, GameRef};

/// The stored Redump DAT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RedumpInfo {
    /// Where the DAT is kept on this machine.
    pub path: String,
    /// The Redump URL, or the file it was imported from.
    pub source: String,
    pub file_name: Option<String>,
    pub description: String,
    /// The DAT's build date, e.g. `2026-10-23 01-56-22`.
    pub version: String,
    pub games: u64,
    /// The DAT carries serials, so bad dumps can be told from unknown games.
    pub serials: bool,
    /// Milliseconds since the Unix epoch.
    pub stored_at: u64,
    /// The last answer from Redump, when it was ever asked.
    pub checked_at: Option<u64>,
    /// Redump is not asked again before this; `None` when it never was.
    pub next_check: Option<u64>,
}

fn millis(secs: u64) -> u64 {
    secs.saturating_mul(1000)
}

fn info(store: &RedumpStore, dat: &DatInfo) -> RedumpInfo {
    // An imported DAT names the file it came from.
    let source = if dat.source.starts_with("http") {
        dat.source.clone()
    } else {
        display_path(Path::new(&dat.source))
    };
    RedumpInfo {
        path: display_path(&store.dat_path()),
        source,
        file_name: dat.file_name.clone(),
        description: dat.description.clone(),
        version: dat.version.clone(),
        games: dat.games,
        serials: dat.serials,
        stored_at: millis(dat.stored_at),
        checked_at: store.last_asked().map(millis),
        next_check: store.next_check().map(millis),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum RedumpUpdate {
    /// A new DAT was downloaded.
    Downloaded,
    /// Redump said nothing changed.
    NotModified,
    /// Redump was asked less than a week ago; no request was made.
    TooSoon,
    /// A file the user had was imported.
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RedumpResult {
    pub update: RedumpUpdate,
    /// The DAT now stored; none when Redump was asked less than a week ago
    /// and that request brought none.
    pub dat: Option<RedumpInfo>,
    /// When Redump may be asked again (milliseconds since the Unix epoch).
    pub next_check: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum VerifyStatus {
    /// Size and SHA-1 equal a Redump dump.
    Verified,
    /// Redump lists the game ID, and no dump of it has these hashes.
    BadDump,
    /// Nothing to compare with (`reason` says why).
    Unknown,
    /// The image could not be read.
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum UnknownReason {
    /// No Redump DAT is stored yet.
    NoDat,
    /// The DAT has no entry with this game ID.
    NotListed,
    /// Redump lists the game as BIN/CUE: an ISO cannot hash like the BIN.
    CdImage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HashValues {
    pub size: u64,
    pub crc32: String,
    pub sha1: String,
    pub md5: Option<String>,
}

impl From<&Hashes> for HashValues {
    fn from(hashes: &Hashes) -> Self {
        Self {
            size: hashes.size,
            crc32: hashes.crc32_hex(),
            sha1: hashes.sha1_hex(),
            md5: hashes.md5_hex(),
        }
    }
}

/// A Redump entry, for the verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RedumpEntry {
    pub name: String,
    /// The disc's page on redump.info.
    pub url: Option<String>,
    pub serial: Option<String>,
    pub version: Option<String>,
    /// The `.iso` or `.bin` the image is compared with.
    pub rom: String,
    pub size: u64,
    /// Same size as the image (for a CD: the same number of sectors).
    pub same_size: bool,
}

impl From<&Candidate> for RedumpEntry {
    fn from(c: &Candidate) -> Self {
        Self {
            name: c.name.clone(),
            url: c
                .id
                .as_ref()
                .map(|id| format!("https://redump.info/disc/{id}")),
            serial: c.serial.clone(),
            version: c.version.clone(),
            rom: c.rom.clone(),
            size: c.size,
            same_size: c.same_size,
        }
    }
}

/// The outcome for one game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GameVerification {
    pub title: String,
    pub startup: String,
    /// The image, or `ul.cfg` for `ul` games.
    pub path: String,
    pub ul_index: Option<u32>,
    pub media: Media,
    pub format: GameFormat,
    pub status: VerifyStatus,
    pub reason: Option<UnknownReason>,
    pub hashes: Option<HashValues>,
    /// The matching dump (verified).
    pub redump: Option<RedumpEntry>,
    /// The game ID is one of the matching entry's serials.
    pub serial_matches: Option<bool>,
    /// Entries with this game ID (bad dump, CD image).
    pub candidates: Vec<RedumpEntry>,
    /// The hashes came from the cache: the image was not read again.
    pub cached: bool,
    pub error: Option<String>,
    /// `error` for front ends that translate it.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_phrase: Option<Phrase>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub games: Vec<GameVerification>,
    /// The DAT the games were compared with.
    pub dat: Option<RedumpInfo>,
}

/// What the verdict says, for the wire.
pub(crate) struct Outcome {
    pub status: VerifyStatus,
    pub reason: Option<UnknownReason>,
    pub redump: Option<RedumpEntry>,
    pub serial_matches: Option<bool>,
    pub candidates: Vec<RedumpEntry>,
}

pub(crate) fn outcome(index: Option<&RedumpIndex>, hashes: &Hashes, startup: &str) -> Outcome {
    let Some(index) = index else {
        return Outcome {
            status: VerifyStatus::Unknown,
            reason: Some(UnknownReason::NoDat),
            redump: None,
            serial_matches: None,
            candidates: Vec::new(),
        };
    };
    match index.verify(hashes, Some(startup), Data::Iso) {
        Verdict::Verified {
            game,
            serial_matches,
        } => Outcome {
            status: VerifyStatus::Verified,
            reason: None,
            redump: Some((&game).into()),
            serial_matches: Some(serial_matches),
            candidates: Vec::new(),
        },
        Verdict::BadDump(candidates) => Outcome {
            status: VerifyStatus::BadDump,
            reason: None,
            redump: None,
            serial_matches: None,
            candidates: candidates.iter().map(Into::into).collect(),
        },
        Verdict::Unknown(Unknown::NotListed) => Outcome {
            status: VerifyStatus::Unknown,
            reason: Some(UnknownReason::NotListed),
            redump: None,
            serial_matches: None,
            candidates: Vec::new(),
        },
        Verdict::Unknown(Unknown::CdImage(candidates)) => Outcome {
            status: VerifyStatus::Unknown,
            reason: Some(UnknownReason::CdImage),
            redump: None,
            serial_matches: None,
            candidates: candidates.iter().map(Into::into).collect(),
        },
    }
}

impl Net {
    pub(crate) fn redump_store(&self) -> RedumpStore {
        RedumpStore::new(self.data_dir())
    }

    /// The stored DAT and its index, parsed once per version and kept in
    /// memory; both from one read, so a report never names one DAT while
    /// its verdicts come from another. `None` before the first download or
    /// import, and for a DAT that does not parse: it is of no use, and the
    /// user can download or import it again.
    pub(crate) fn redump(&self) -> Option<(RedumpInfo, Arc<RedumpIndex>)> {
        let store = self.redump_store();
        let dat = store.info().ok().flatten()?;
        let mut cached = self.redump.lock().unwrap_or_else(PoisonError::into_inner);
        let index = match cached.as_ref() {
            Some((sha256, index)) if *sha256 == dat.sha256 => Arc::clone(index),
            _ => {
                let bytes = std::fs::read(store.dat_path()).ok()?;
                let parsed = yaoplm_hash::dat::parse(&bytes).ok()?;
                let index = Arc::new(RedumpIndex::new(parsed));
                *cached = Some((dat.sha256.clone(), Arc::clone(&index)));
                index
            }
        };
        Some((info(&store, &dat), index))
    }

    pub(crate) fn redump_info(&self) -> Option<RedumpInfo> {
        self.redump().map(|(info, _)| info)
    }
}

/// The body of an `UpdateRedump` job: a download, or an import of `file`.
pub(crate) fn update_job(net: &Net, file: Option<PathBuf>) -> Result<JobResult, ApiError> {
    let _one_at_a_time = net
        .redump_update
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let store = net.redump_store();
    let (update, dat) = match file {
        Some(path) => {
            let dat = store
                .import(&path)
                .map_err(|e| net_error(ErrorCode::BadRequest, &e))?;
            (RedumpUpdate::Imported, Some(dat))
        }
        None => {
            let service = net.service()?;
            match net.block_on(store.update(service.http()))? {
                Ok(Update::Stored(dat)) => (RedumpUpdate::Downloaded, Some(dat)),
                Ok(Update::NotModified(dat)) => (RedumpUpdate::NotModified, Some(dat)),
                Ok(Update::TooSoon { info, .. }) => (RedumpUpdate::TooSoon, info),
                Err(err) => return Err(net_error(ErrorCode::Internal, &err)),
            }
        }
    };
    Ok(JobResult::Redump(Box::new(RedumpResult {
        update,
        dat: dat.map(|d| info(&store, &d)),
        next_check: store.next_check().map(millis),
    })))
}

pub(crate) struct VerifyParams {
    pub root: PathBuf,
    pub profile: LoaderProfile,
    /// Images (relative paths, lower case) and `ul.cfg` records to verify;
    /// `None` means every game.
    pub games: Option<Vec<Wanted>>,
    pub md5: bool,
    /// Read every image even when its hashes are cached.
    pub fresh: bool,
}

/// A game a request names.
pub(crate) enum Wanted {
    Image(String),
    Ul { index: u32, startup: String },
}

/// `CD/Game.iso`, lower case, for comparing with a request.
fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect();
    Some(parts.join("/"))
}

fn names(root: &Path, game: &CoreGame, wanted: &Wanted) -> bool {
    match (wanted, &game.location) {
        (Wanted::Image(rel), GameLocation::Image { path, .. }) => {
            relative(root, path).as_deref() == Some(rel.as_str())
        }
        (Wanted::Ul { index, startup }, GameLocation::Ul { index: i, .. }) => {
            u32::try_from(*i).ok() == Some(*index) && game.startup == *startup
        }
        _ => false,
    }
}

/// What tells the volume at `root` from another one mounted at the same
/// place, for the hash cache: serial, file system and size.
fn volume_key(root: &Path) -> String {
    yaoplm_device::volume_for_path(root)
        .map(|v| {
            format!(
                "{}|{}|{}",
                v.serial.as_deref().unwrap_or("-"),
                v.fs_name,
                v.total_bytes
            )
        })
        .unwrap_or_default()
}

/// The files of `game` with their times, for the hash cache.
fn hashed_files(game: &CoreGame) -> Result<Vec<(PathBuf, u64, Option<u128>)>, std::io::Error> {
    yaoplm_core::verify::image_files(game)?
        .into_iter()
        .map(|(path, len)| {
            let modified = std::fs::metadata(&path)?
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos());
            Ok((path, len, modified))
        })
        .collect()
}

fn keys<'a>(volume: &'a str, files: &'a [(PathBuf, u64, Option<u128>)]) -> Vec<HashedFile<'a>> {
    files
        .iter()
        .map(|(path, len, modified)| HashedFile {
            volume,
            path,
            len: *len,
            modified: *modified,
        })
        .collect()
}

/// The body of a `VerifyGames` job.
pub(crate) fn verify_job(
    ctx: &JobContext,
    net: &Net,
    params: VerifyParams,
) -> Result<JobResult, ApiError> {
    let options = ScanOptions {
        profile: params.profile.into(),
        verify_images: false,
    };
    let library =
        yaoplm_core::scan_library(&params.root, &options).map_err(|e| crate::scan_error(&e))?;
    if let Some(list) = &params.games
        && let Some(missing) = list
            .iter()
            .position(|w| !library.games.iter().any(|g| names(&library.root, g, w)))
    {
        return Err(ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("gameNotInRequest").with("index", missing + 1),
        ));
    }
    let games: Vec<&CoreGame> = library
        .games
        .iter()
        .filter(|g| {
            params
                .games
                .as_ref()
                .is_none_or(|list| list.iter().any(|w| names(&library.root, g, w)))
        })
        .collect();
    if games.is_empty() {
        return Err(ApiError::phrased(
            ErrorCode::NotFound,
            &Said::error("noGameToVerify"),
        ));
    }
    let (dat, index) = net.redump().unzip();
    // The cache only saves time: without it every image is read.
    let service = net.service().ok();
    let cache = service.as_ref().map(|s| s.cache());
    let volume = volume_key(&library.root);
    let algorithms = Algorithms { md5: params.md5 };

    // First the cache, so the progress total counts only what is read.
    let mut work = Vec::new();
    let mut total = 0u64;
    for game in &games {
        let files = hashed_files(game);
        let cached = match (&files, cache) {
            (Ok(files), Some(cache)) if !params.fresh => cache
                .hashes(&keys(&volume, files), params.md5)
                .ok()
                .flatten(),
            _ => None,
        };
        if cached.is_none()
            && let Ok(files) = &files
        {
            total += files.iter().map(|(_, len, _)| len).sum::<u64>();
        }
        work.push((game, files, cached));
    }

    let mut done = 0u64;
    let mut results = Vec::with_capacity(work.len());
    ctx.progress(0, Some(total), None);
    for (game, files, cached) in work {
        if ctx.is_cancelled() {
            return Err(ApiError::cancelled());
        }
        let file_bytes: u64 = files
            .as_ref()
            .map_or(0, |f| f.iter().map(|(_, len, _)| len).sum());
        let from_cache = cached.is_some();
        let hashed: Result<Hashes, String> = match (cached, &files) {
            (Some(hashes), _) => Ok(hashes),
            (None, Err(err)) => Err(err.to_string()),
            (None, Ok(list)) => {
                let start = done;
                let result = yaoplm_core::verify::hash_game(
                    game,
                    algorithms,
                    &|| ctx.is_cancelled(),
                    &mut |read, of| {
                        let part = if of == 0 {
                            0
                        } else {
                            (u128::from(read.min(of)) * u128::from(file_bytes) / u128::from(of))
                                as u64
                        };
                        ctx.progress(start + part, Some(total), Some(&game.title));
                    },
                );
                done = start + file_bytes;
                match result {
                    Ok(hashes) => {
                        // The cache only saves time; a failure costs a re-read.
                        if let Some(cache) = cache {
                            let _ = cache.put_hashes(&keys(&volume, list), &hashes);
                        }
                        Ok(hashes)
                    }
                    Err(HashError::Cancelled) => {
                        return Err(ApiError::cancelled());
                    }
                    Err(HashError::Io(err)) => Err(err.to_string()),
                }
            }
        };
        let (location_path, ul_index) = match &game.location {
            GameLocation::Image { path, .. } => (display_path(path), None),
            GameLocation::Ul { index, .. } => (
                display_path(&library.root.join("ul.cfg")),
                u32::try_from(*index).ok(),
            ),
        };
        let entry = match hashed {
            Ok(hashes) => {
                let o = outcome(index.as_deref(), &hashes, &game.startup);
                GameVerification {
                    title: game.title.clone(),
                    startup: game.startup.clone(),
                    path: location_path,
                    ul_index,
                    media: game.media.into(),
                    format: game.format.into(),
                    status: o.status,
                    reason: o.reason,
                    hashes: Some((&hashes).into()),
                    redump: o.redump,
                    serial_matches: o.serial_matches,
                    candidates: o.candidates,
                    cached: from_cache,
                    error: None,
                    error_phrase: None,
                }
            }
            Err(error) => {
                let phrase = Said::notice("unreadable").with("error", &error);
                GameVerification {
                    title: game.title.clone(),
                    startup: game.startup.clone(),
                    path: location_path,
                    ul_index,
                    media: game.media.into(),
                    format: game.format.into(),
                    status: VerifyStatus::Failed,
                    reason: None,
                    hashes: None,
                    redump: None,
                    serial_matches: None,
                    candidates: Vec::new(),
                    cached: false,
                    error: Some(phrase.english()),
                    error_phrase: Some(Phrase::from(&phrase)),
                }
            }
        };
        ctx.log(JobLogEntry::Verify {
            at: now_ms(),
            startup: entry.startup.clone(),
            title: entry.title.clone(),
            status: entry.status,
            reason: entry.reason,
            redump: entry.redump.as_ref().map(|r| r.name.clone()),
            cached: entry.cached,
            error: entry.error.clone(),
            error_phrase: entry.error_phrase.clone(),
        });
        results.push(entry);
    }
    ctx.progress(total, Some(total), None);
    Ok(JobResult::Verification(Box::new(VerificationReport {
        games: results,
        dat,
    })))
}

/// Fills `Game::dump` from the hash cache for games whose files did not
/// change since they were hashed. No image is read; when the cache cannot
/// be opened the library simply has no verdicts.
pub(crate) fn attach_checks(
    wire: &mut crate::library::Library,
    core: &yaoplm_core::Library,
    net: &Net,
) {
    let Ok(Some(cache)) = Cache::open_existing(net.data_dir()) else {
        return;
    };
    let index = net.redump().map(|(_, index)| index);
    let volume = volume_key(&core.root);
    for (game, core_game) in wire.games.iter_mut().zip(&core.games) {
        let Ok(files) = hashed_files(core_game) else {
            continue;
        };
        let Ok(Some(hashes)) = cache.hashes(&keys(&volume, &files), false) else {
            continue;
        };
        let o = outcome(index.as_deref(), &hashes, &core_game.startup);
        game.dump = Some(crate::library::DumpCheck {
            status: o.status,
            reason: o.reason,
            redump: o.redump.map(|r| r.name),
        });
    }
}

/// Turns request game references into what the job compares.
pub(crate) fn wanted_list(root: &Path, games: &[GameRef]) -> Vec<Wanted> {
    games
        .iter()
        .map(|g| match g {
            GameRef::Image { path } => {
                let candidate = Path::new(path);
                let rel = if candidate.is_absolute() {
                    std::fs::canonicalize(candidate)
                        .ok()
                        .and_then(|p| relative(root, &p))
                        .unwrap_or_default()
                } else {
                    path.replace('\\', "/").to_lowercase()
                };
                Wanted::Image(rel)
            }
            GameRef::Ul { index, startup, .. } => Wanted::Ul {
                index: *index,
                startup: startup.clone(),
            },
        })
        .collect()
}
