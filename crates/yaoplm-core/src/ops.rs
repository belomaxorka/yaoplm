//! Planners for changing games already on the device: rename (GAME-1),
//! delete (GAME-2), move between CD and DVD (GAME-3), and the folder set-up
//! (LIB-3). Every CFG that is moved or removed is first copied to
//! `.yaoplm/backup/` (CFG-5).
//!
//! What OPL ties to what (research/02 §4-§9, research/09 §3):
//! - CFG, ART and CHT files are named after the game ID, VMC cards after
//!   whatever `$VMC_0`/`$VMC_1` say. A new title therefore changes only the
//!   image name, or the `ul.cfg` record and the chunk names (their hash
//!   comes from the title). Only a new ID moves CFG, art and cheats.
//! - VMC cards are never renamed or deleted: the CFG keeps pointing at
//!   them, and cards are often shared by a series or `generic_*` (ADR-0020).
//! - `sbRename()` in ps2homebrew/Open-PS2-Loader@b3a21997 renames the
//!   chunks and rewrites `ul.cfg` without checking errors; here the chunk
//!   renames come first, then `ul.cfg` atomically, and a failure undoes them.
//! - `ul.cfg` keeps every byte of the other records, and its ragged tail.
//! - FAT matches names case-insensitively, so a case-only rename goes
//!   through a temporary name.

use std::collections::HashSet;
use std::io;

use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, DirEntry, TMP_SUFFIX, find_entry, list_or_empty};
use yaoplm_formats::art::parse_art_file_name;
use yaoplm_formats::image::{self, DiscImage};
use yaoplm_formats::iso_name::{self, ImageExtension, IsoFileName, NameFormat, NameStyle};
use yaoplm_formats::ul_cfg::{RECORD_SIZE, UlRecord};
use yaoplm_formats::{Media, games_bin};
use yaoplm_i18n::Phrase;

use crate::device::{DeviceState, join, rel};
use crate::import::image_name;
use crate::journal::{self, BACKUP_DIR};
use crate::plan::{Bytes, Expect, GameInfo, Notice, Plan, PlanKind, Removal, Step, Transaction};
use crate::reasons;
use crate::scan::OPL_FOLDERS;

/// Which game a request is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameRef {
    /// An image in `CD/` or `DVD/`.
    Image { path: RelPath },
    /// Record `index` of `ul.cfg`; `startup` and `name` must still match,
    /// so a changed `ul.cfg` never hits the wrong game.
    Ul {
        index: usize,
        startup: String,
        name: String,
    },
}

/// Why a plan cannot be made: a phrase front ends translate, whose
/// English text is the `Display` (ADR-0029).
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error("{0}")]
    NotFound(Phrase),
    #[error("{0}")]
    Invalid(Phrase),
    #[error("{0}")]
    Conflict(Phrase),
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl OpError {
    pub fn phrase(&self) -> Phrase {
        match self {
            Self::NotFound(phrase) | Self::Invalid(phrase) | Self::Conflict(phrase) => {
                phrase.clone()
            }
            Self::Io(err) => Phrase::error("system").with("error", err.to_string()),
        }
    }
}

fn invalid(phrase: Phrase) -> OpError {
    OpError::Invalid(phrase)
}

/// A game as the planners need it.
pub(crate) struct Found {
    pub(crate) media: Media,
    pub(crate) kind: FoundKind,
    /// The ID OPL uses for CFG, ART and CHT, when it could be read.
    pub(crate) startup: Option<String>,
}

pub(crate) enum FoundKind {
    Image {
        path: RelPath,
        name: IsoFileName,
        len: u64,
    },
    Ul {
        index: usize,
        record: UlRecord,
    },
}

fn image_startup(fs: &dyn DeviceFs, path: &RelPath) -> Option<String> {
    let reader = fs.open(path).ok()?;
    let mut image = DiscImage::open(reader).ok()?;
    image::opl_startup(&mut image).ok()
}

fn media_of_folder(state: &DeviceState, folder: &str) -> Option<Media> {
    [Media::Cd, Media::Dvd].into_iter().find(|m| {
        state
            .folder(*m)
            .is_ok_and(|f| f.as_str().eq_ignore_ascii_case(folder))
    })
}

pub(crate) fn find_game(
    fs: &dyn DeviceFs,
    state: &DeviceState,
    game: &GameRef,
) -> Result<Found, OpError> {
    match game {
        GameRef::Image { path } => {
            let folder = path.parent().filter(|p| !p.is_root()).ok_or_else(|| {
                invalid(Phrase::error("notInMediaFolder").with("path", path.to_string()))
            })?;
            let media = media_of_folder(state, folder.as_str()).ok_or_else(|| {
                invalid(Phrase::error("notInMediaFolder").with("path", path.to_string()))
            })?;
            let entry = find_entry(state.entries(media), path.file_name())
                .filter(|e| !e.is_dir)
                .ok_or_else(|| {
                    OpError::NotFound(Phrase::error("doesNotExist").with("path", path.to_string()))
                })?;
            let path = join(&state.folder(media)?, &entry.name)?;
            let name = iso_name::classify(&entry.name).map_err(|e| {
                invalid(
                    Phrase::error("notListedByOpl")
                        .with("path", path.to_string())
                        .with("reason", e.to_string()),
                )
            })?;
            let startup = match &name.startup {
                Some(id) => Some(id.clone()),
                None => image_startup(fs, &path),
            };
            Ok(Found {
                media,
                startup,
                kind: FoundKind::Image {
                    path,
                    name,
                    len: entry.len,
                },
            })
        }
        GameRef::Ul {
            index,
            startup,
            name,
        } => {
            let record = state
                .ul
                .records
                .get(*index)
                .filter(|r| r.startup() == *startup && r.name() == *name)
                .ok_or_else(|| {
                    OpError::NotFound(
                        Phrase::error("ulRecordMoved")
                            .with("name", name)
                            .with("startup", startup)
                            .with("index", *index),
                    )
                })?;
            Ok(Found {
                media: record.media(),
                startup: Some(record.startup()),
                kind: FoundKind::Ul {
                    index: *index,
                    record: record.clone(),
                },
            })
        }
    }
}

/// IDs of every other game on the device, as OPL derives them.
fn other_ids(
    fs: &dyn DeviceFs,
    state: &DeviceState,
    except: &Found,
) -> io::Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for media in [Media::Cd, Media::Dvd] {
        let folder = state.folder(media)?;
        let cache = find_entry(state.entries(media), games_bin::FILE_NAME)
            .and_then(|e| fs.read(&join(&folder, &e.name).ok()?).ok())
            .map(|bytes| games_bin::parse(&bytes))
            .unwrap_or_default();
        for entry in state.entries(media).iter().filter(|e| !e.is_dir) {
            let path = join(&folder, &entry.name)?;
            if matches!(&except.kind, FoundKind::Image { path: p, .. } if *p == path) {
                continue;
            }
            let Ok(name) = iso_name::classify(&entry.name) else {
                continue;
            };
            let id = match name.startup {
                Some(id) => Some(id),
                None => games_bin::lookup(&cache, &entry.name)
                    .map(|c| c.startup.clone())
                    .or_else(|| image_startup(fs, &path)),
            };
            ids.extend(id);
        }
    }
    for (index, record) in state.ul.records.iter().enumerate() {
        if !matches!(except.kind, FoundKind::Ul { index: i, .. } if i == index) {
            ids.insert(record.startup());
        }
    }
    Ok(ids)
}

/// Files named after `startup`: `CFG/<ID>.cfg` (and wOPL's `.info`),
/// `ART/<ID>_*`, `CHT/<ID>.cht`.
fn related_files(
    fs: &dyn DeviceFs,
    state: &DeviceState,
    startup: &str,
) -> io::Result<Vec<(RelPath, DirEntry)>> {
    let mut found = Vec::new();
    let mut scan = |folder: &str, matches: &dyn Fn(&str) -> bool| -> io::Result<()> {
        let Some(dir) = find_entry(&state.top, folder).filter(|e| e.is_dir) else {
            return Ok(());
        };
        let dir = rel(&dir.name)?;
        for entry in list_or_empty(fs, &dir)? {
            if !entry.is_dir && matches(&entry.name) {
                found.push((join(&dir, &entry.name)?, entry));
            }
        }
        Ok(())
    };
    let lower = startup.to_lowercase();
    scan("CFG", &|n| {
        let n = n.to_lowercase();
        n == format!("{lower}.cfg") || n == format!("{lower}.info")
    })?;
    scan("ART", &|n| {
        parse_art_file_name(n).is_some_and(|a| a.key.to_lowercase() == lower)
    })?;
    scan("CHT", &|n| n.to_lowercase() == format!("{lower}.cht"))?;
    Ok(found)
}

/// `CFG/SLUS_200.62.cfg` → `CFG/SLES_500.51.cfg`, keeping the suffix.
fn renamed_related(path: &RelPath, entry: &DirEntry, old: &str, new: &str) -> io::Result<RelPath> {
    let rest = entry.name.get(old.len()..).unwrap_or("");
    join(&path.parent().unwrap_or_default(), &format!("{new}{rest}"))
}

pub(crate) fn backup_step(backup: &RelPath, path: &RelPath) -> io::Result<Step> {
    Ok(Step::CopyFile {
        from: path.clone(),
        to: rel(&format!("{}/{}", backup.as_str(), path.as_str()))?,
    })
}

pub(crate) fn new_backup_dir() -> io::Result<RelPath> {
    rel(&format!("{BACKUP_DIR}/{}", journal::new_id()))
}

/// Renames `from` to `to`; a case-only change goes through a temporary name.
fn rename_steps(from: &RelPath, to: &RelPath) -> io::Result<Vec<Step>> {
    if from.as_str() != to.as_str() && from.as_str().to_lowercase() == to.as_str().to_lowercase() {
        let tmp = from
            .with_suffix(TMP_SUFFIX)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        return Ok(vec![
            Step::Rename {
                from: from.clone(),
                to: tmp.clone(),
            },
            Step::Rename {
                from: tmp,
                to: to.clone(),
            },
        ]);
    }
    Ok(vec![Step::Rename {
        from: from.clone(),
        to: to.clone(),
    }])
}

fn ul_cfg_with(state: &DeviceState, index: usize, record: Option<&UlRecord>) -> io::Result<Step> {
    let current = state.ul_bytes.clone().unwrap_or_default();
    let mut next = current.clone();
    let at = index * RECORD_SIZE;
    match record {
        Some(record) => next[at..at + RECORD_SIZE].copy_from_slice(record.as_bytes()),
        None => {
            next.drain(at..at + RECORD_SIZE);
        }
    }
    Ok(Step::PutFile {
        path: state.ul_path()?,
        content: Bytes(next),
        expect: Expect::Content(Bytes(current)),
    })
}

pub(crate) fn game_info(
    title: &str,
    startup: Option<&str>,
    media: Media,
    format: &str,
) -> GameInfo {
    GameInfo {
        title: title.to_owned(),
        startup: startup.unwrap_or("").to_owned(),
        media,
        format: format.to_owned(),
    }
}

pub(crate) fn new_plan(kind: PlanKind, title: Phrase, state: &DeviceState) -> Plan {
    let mut plan = Plan::titled(
        kind,
        title,
        state.info.fingerprint.clone(),
        state.info.free_bytes,
    );
    plan.notices.extend(state.unfinished_notice());
    plan
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenameOptions {
    pub title: String,
    /// A corrected game ID (old-style names and `ul` only). CFG, art and
    /// cheats move with it.
    pub startup: Option<String>,
    /// Switch between `ID.Title.iso` and `Title.iso`; `None` keeps it.
    pub name_style: Option<NameStyle>,
}

/// Plans a rename (GAME-1).
pub fn plan_rename(
    fs: &dyn DeviceFs,
    game: &GameRef,
    options: &RenameOptions,
) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let found = find_game(fs, &state, game)?;
    let mut tx = Transaction::new(String::new());
    let mut notices = Vec::new();
    let old_id = found.startup.clone();
    let (new_id, title) = match &found.kind {
        FoundKind::Image { path, name, .. } => {
            let style = options.name_style.unwrap_or(match name.format {
                NameFormat::Old => NameStyle::WithStartup,
                NameFormat::New => NameStyle::TitleOnly,
            });
            let image_id = match name.format {
                NameFormat::New => old_id.clone(),
                NameFormat::Old => image_startup(fs, path),
            };
            let new_id = match (style, &options.startup) {
                (NameStyle::WithStartup, Some(id)) => id.clone(),
                (NameStyle::WithStartup, None) => old_id
                    .clone()
                    .ok_or_else(|| invalid(Phrase::error("noGameId")))?,
                (NameStyle::TitleOnly, Some(_)) => {
                    return Err(invalid(Phrase::error("titleOnlyNoId")));
                }
                (NameStyle::TitleOnly, None) => image_id
                    .clone()
                    .ok_or_else(|| invalid(Phrase::error("noGameId")))?,
            };
            if style == NameStyle::WithStartup && !iso_name::is_name_safe_startup(&new_id) {
                return Err(invalid(
                    Phrase::error("idCannotStartName").with("id", &new_id),
                ));
            }
            let clean = iso_name::sanitize_title(&options.title);
            if clean.is_empty() {
                return Err(invalid(Phrase::error("emptyTitle")));
            }
            let ext = name.extension;
            let new_name = image_name(&new_id, &clean, ext, style, &mut notices)
                .map_err(|e| invalid(reasons::name(&e)))?;
            let folder = path.parent().unwrap_or_default();
            let target = join(&folder, &new_name)?;
            if target.as_str() == path.as_str() {
                return Err(invalid(Phrase::error("nameUnchanged")));
            }
            let case_only = target.as_str().to_lowercase() == path.as_str().to_lowercase();
            if !case_only && find_entry(state.entries(found.media), &new_name).is_some() {
                return Err(OpError::Conflict(
                    Phrase::error("alreadyExists").with("path", target.to_string()),
                ));
            }
            if ext == ImageExtension::Zso && name.raw_extension != ".zso" {
                notices.push(Notice::info(Phrase::notice("extensionLowercased")));
            }
            for step in rename_steps(path, &target)? {
                tx.push(step);
            }
            tx.set_title(
                Phrase::title("renameFile")
                    .with("from", path.file_name())
                    .with("to", &new_name),
            );
            tx.game = Some(game_info(
                &iso_name::classify(&new_name).map_or(clean, |n| n.title),
                Some(&new_id),
                found.media,
                if ext == ImageExtension::Zso {
                    "ZSO"
                } else {
                    "ISO"
                },
            ));
            (Some(new_id), new_name)
        }
        FoundKind::Ul { index, record } => {
            let title = options.title.clone();
            let mut renamed = record
                .renamed(&title)
                .map_err(|e| invalid(reasons::ul(&e)))?;
            if let Some(id) = &options.startup {
                renamed = renamed
                    .with_startup(id)
                    .map_err(|e| invalid(reasons::ul(&e)))?;
            }
            if renamed == *record {
                return Err(invalid(Phrase::error("nameUnchanged")));
            }
            for part in 0..record.parts() {
                let (from, to) = (record.chunk_name(part), renamed.chunk_name(part));
                if from == to {
                    continue;
                }
                if find_entry(&state.top, &from).is_none() {
                    return Err(OpError::NotFound(
                        Phrase::error("chunkMissing").with("name", &from),
                    ));
                }
                if find_entry(&state.top, &to).is_some() {
                    return Err(OpError::Conflict(
                        Phrase::error("alreadyExists").with("path", &to),
                    ));
                }
                for step in rename_steps(&rel(&from)?, &rel(&to)?)? {
                    tx.push(step);
                }
            }
            tx.push(ul_cfg_with(&state, *index, Some(&renamed))?);
            if state.info.smb {
                notices.push(Notice::info(Phrase::notice("restartOpl")));
            }
            tx.set_title(
                Phrase::title("renameFile")
                    .with("from", record.name())
                    .with("to", &title),
            );
            tx.game = Some(game_info(
                &title,
                Some(&renamed.startup()),
                found.media,
                "UL",
            ));
            (Some(renamed.startup()), title)
        }
    };

    if let (Some(old), Some(new)) = (&old_id, &new_id)
        && old != new
    {
        let others = other_ids(fs, &state, &found)?;
        let shared = others.contains(old);
        let related = related_files(fs, &state, old)?;
        let backup = new_backup_dir()?;
        for (path, entry) in &related {
            let target = renamed_related(path, entry, old, new)?;
            if fs.metadata(&target)?.is_some() {
                return Err(OpError::Conflict(
                    Phrase::error("existsMoveFirst").with("path", target.to_string()),
                ));
            }
            if path.as_str().to_lowercase().ends_with(".cfg") {
                tx.push(backup_step(&backup, path)?);
            }
            if shared {
                tx.push(Step::CopyFile {
                    from: path.clone(),
                    to: target,
                });
            } else {
                tx.push(Step::Rename {
                    from: path.clone(),
                    to: target,
                });
            }
        }
        if !related.is_empty() {
            notices.push(Notice::info(
                Phrase::notice(if shared {
                    "relatedCopied"
                } else {
                    "relatedMoved"
                })
                .with("old", old)
                .with("new", new)
                .with("files", related.len()),
            ));
        }
    }
    if let FoundKind::Image { .. } = found.kind
        && let Some(cache) = state.drop_cache(found.media)?
    {
        tx.then(cache);
    }
    tx.notices = notices;
    let mut plan = new_plan(
        PlanKind::Rename,
        Phrase::title("renameGame").with("title", &title),
        &state,
    );
    plan.transactions.push(tx);
    Ok(plan)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeleteOptions {
    /// Also delete `CFG/<ID>.cfg`, `ART/<ID>_*` and `CHT/<ID>.cht`, unless
    /// another game uses the ID. VMC cards are never deleted.
    pub related: bool,
}

/// Plans deleting a game (GAME-2). Executing it needs explicit confirmation.
pub fn plan_delete(
    fs: &dyn DeviceFs,
    game: &GameRef,
    options: &DeleteOptions,
) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let found = find_game(fs, &state, game)?;
    let mut tx = Transaction::new(String::new());
    let mut notices = Vec::new();
    let title = match &found.kind {
        FoundKind::Image { path, name, len } => {
            tx.then(Step::Remove {
                path: path.clone(),
                len: *len,
                what: Removal::UserData,
            });
            if let Some(cache) = state.drop_cache(found.media)? {
                tx.then(cache);
            }
            tx.game = Some(game_info(
                &name.title,
                found.startup.as_deref(),
                found.media,
                if name.extension == ImageExtension::Zso {
                    "ZSO"
                } else {
                    "ISO"
                },
            ));
            name.title.clone()
        }
        FoundKind::Ul { index, record } => {
            tx.push(ul_cfg_with(&state, *index, None)?);
            for part in 0..record.parts() {
                let chunk = record.chunk_name(part);
                if let Some(entry) = find_entry(&state.top, &chunk) {
                    tx.then(Step::Remove {
                        path: rel(&entry.name)?,
                        len: entry.len,
                        what: Removal::UserData,
                    });
                }
            }
            tx.game = Some(game_info(
                &record.name(),
                Some(&record.startup()),
                found.media,
                "UL",
            ));
            record.name()
        }
    };
    if let Some(startup) = &found.startup {
        let related = related_files(fs, &state, startup)?;
        if options.related && !related.is_empty() {
            if other_ids(fs, &state, &found)?.contains(startup) {
                notices.push(Notice::info(
                    Phrase::notice("relatedShared").with("startup", startup),
                ));
            } else {
                let backup = new_backup_dir()?;
                for (path, entry) in &related {
                    if path.as_str().to_lowercase().ends_with(".cfg") {
                        let step = backup_step(&backup, path)?;
                        // Backups happen before the commit point.
                        tx.steps.insert(tx.commit, step);
                        tx.commit += 1;
                    }
                    tx.then(Step::Remove {
                        path: path.clone(),
                        len: entry.len,
                        what: Removal::UserData,
                    });
                }
            }
        } else if !related.is_empty() {
            notices.push(Notice::info(
                Phrase::notice("relatedKept")
                    .with("startup", startup)
                    .with("files", related.len()),
            ));
        }
        notices.push(Notice::info(Phrase::notice("vmcKept")));
    }
    let archived = ["ART/art.tar", "CHT/cht.tar"]
        .iter()
        .any(|p| rel(p).is_ok_and(|p| fs.metadata(&p).ok().flatten().is_some()));
    if archived {
        notices.push(Notice::info(Phrase::notice("archiveKept")));
    }
    let phrase = Phrase::title("deleteGame").with("title", &title);
    tx.set_title(phrase.clone());
    tx.notices = notices;
    let mut plan = new_plan(PlanKind::Delete, phrase, &state);
    plan.transactions.push(tx);
    Ok(plan)
}

/// Plans moving a game between `CD/` and `DVD/` (GAME-3). For `ul` games
/// only the media byte changes.
pub fn plan_move(fs: &dyn DeviceFs, game: &GameRef, media: Media) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let found = find_game(fs, &state, game)?;
    if found.media == media {
        return Err(invalid(
            Phrase::error("alreadyOnMedia").with("media", media.to_string()),
        ));
    }
    let mut tx = Transaction::new(String::new());
    let title = match &found.kind {
        FoundKind::Image { path, name, .. } => {
            if !state.has_folder(media.folder()) {
                tx.push(Step::CreateDir {
                    path: state.folder(media)?,
                });
            }
            let target = join(&state.folder(media)?, path.file_name())?;
            if find_entry(state.entries(media), path.file_name()).is_some() {
                return Err(OpError::Conflict(
                    Phrase::error("alreadyExists").with("path", target.to_string()),
                ));
            }
            tx.push(Step::Rename {
                from: path.clone(),
                to: target,
            });
            for cache in [state.drop_cache(found.media)?, state.drop_cache(media)?]
                .into_iter()
                .flatten()
            {
                tx.then(cache);
            }
            tx.game = Some(game_info(
                &name.title,
                found.startup.as_deref(),
                media,
                if name.extension == ImageExtension::Zso {
                    "ZSO"
                } else {
                    "ISO"
                },
            ));
            name.title.clone()
        }
        FoundKind::Ul { index, record } => {
            tx.push(ul_cfg_with(
                &state,
                *index,
                Some(&record.with_media(media)),
            )?);
            tx.game = Some(game_info(
                &record.name(),
                Some(&record.startup()),
                media,
                "UL",
            ));
            record.name()
        }
    };
    let phrase = Phrase::title("moveGame")
        .with("title", &title)
        .with("media", media.to_string());
    tx.set_title(phrase.clone());
    let mut plan = new_plan(PlanKind::Move, phrase, &state);
    plan.transactions.push(tx);
    Ok(plan)
}

/// Plans creating the standard OPL folders that are missing (LIB-3).
pub fn plan_init(fs: &dyn DeviceFs) -> io::Result<Plan> {
    let state = DeviceState::read(fs)?;
    let mut plan = new_plan(
        PlanKind::InitLibrary,
        Phrase::title("createFolders"),
        &state,
    );
    let steps = state.missing_folders()?;
    if steps.is_empty() {
        plan.notices
            .push(Notice::info(Phrase::notice("foldersPresent")));
    } else {
        let mut tx = Transaction::titled(Phrase::title("createFolders"));
        for step in steps {
            tx.push(step);
        }
        plan.transactions.push(tx);
    }
    Ok(plan)
}

/// Plans a folder `name` at the root with the standard OPL folders in it: a
/// library of its own, for a whole system disk shared over SMB (ADR-0045).
/// A folder that exists already only gets the OPL folders it lacks.
pub fn plan_init_folder(fs: &dyn DeviceFs, name: &str) -> Result<Plan, OpError> {
    let folder = RelPath::root()
        .join(name.trim())
        .map_err(|e| invalid(e.phrase()))?;
    let state = DeviceState::read(fs)?;
    let title = Phrase::title("createLibraryFolder").with("folder", folder.as_str());
    let mut plan = new_plan(PlanKind::InitLibrary, title.clone(), &state);
    let mut tx = Transaction::titled(title);
    let present = match fs.metadata(&folder)? {
        Some(meta) if !meta.is_dir => {
            return Err(OpError::Conflict(
                Phrase::error("alreadyExists").with("path", folder.as_str()),
            ));
        }
        Some(_) => list_or_empty(fs, &folder)?,
        None => {
            tx.push(Step::CreateDir {
                path: folder.clone(),
            });
            Vec::new()
        }
    };
    for name in OPL_FOLDERS {
        if find_entry(&present, name).is_none_or(|e| !e.is_dir) {
            tx.push(Step::CreateDir {
                path: join(&folder, name)?,
            });
        }
    }
    if tx.steps.is_empty() {
        plan.notices
            .push(Notice::info(Phrase::notice("foldersPresent")));
    } else {
        plan.transactions.push(tx);
    }
    Ok(plan)
}
