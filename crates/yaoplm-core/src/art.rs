//! Plans that put pictures on the device or take them off (ART-1 to
//! ART-4; SPEC §6 "every write is a plan").
//!
//! The covers themselves are PNG files on the host, ready-made by
//! `yaoplm-net` in its cache or supplied by the user; a plan only copies
//! them into `ART/<ID>_<KIND>.png` (`bdmGetImage()`, `src/bdmsupport.c`,
//! ps2homebrew/Open-PS2-Loader@b3a21997). A file that is already there is
//! moved to `.yaoplm/backup/<id>/ART/` first, so "overwrite" can be undone
//! by hand and a rollback puts it back; deleting a cover copies it there
//! and then removes it, which needs the user's explicit confirmation like
//! any other deletion (ADR-0021).
//!
//! One transaction per picture: a failure on one leaves the others in
//! place, and the journal is small because the bytes stay on the host. The
//! import planner adds the same transactions after each game it copies
//! (ART-8, SPEC §5.4.1).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;

use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, DirEntry, find_entry, list_or_empty};
use yaoplm_device::source::SourceFs;
use yaoplm_formats::Media;
use yaoplm_formats::art::{ART_FOLDER, ArtKind, art_file_name, parse_art_file_name};
use yaoplm_i18n::Phrase;

use crate::device::{DeviceState, join, rel};
use crate::journal::{self, BACKUP_DIR};
use crate::ops::OpError;
use crate::plan::{
    GameInfo, Notice, Plan, PlanKind, Removal, Skipped, Source, SourceFile, SourceKind, Step,
    Transaction,
};
use crate::reasons;

/// What a picture type is called (ART-2): "cover", "icon", "spine"...
pub fn kind_name(kind: ArtKind) -> Phrase {
    match kind {
        ArtKind::Cov => Phrase::reason("artCov"),
        ArtKind::Cov2 => Phrase::reason("artCov2"),
        ArtKind::Lab => Phrase::reason("artLab"),
        ArtKind::Ico => Phrase::reason("artIco"),
        ArtKind::Lgo => Phrase::reason("artLgo"),
        ArtKind::Scr => Phrase::reason("artScr"),
        ArtKind::Scr2 => Phrase::reason("artScr2"),
        ArtKind::Bg => Phrase::reason("artBg"),
    }
}

/// A cover to write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtItem {
    pub startup: String,
    /// For the plan screen.
    pub title: String,
    pub media: Media,
    /// `ISO`, `ZSO` or `UL`, for the plan screen.
    pub format: String,
    pub kind: ArtKind,
    /// The PNG on the host, already the right size (ART-2).
    pub file: PathBuf,
    /// Where it came from, for the plan screen and the log.
    /// Where it comes from, in a word: a picture source, a set, a file.
    pub source: String,
    /// The original in full: its URL, mirror path or file.
    pub origin: String,
}

/// The art folder as named on the device, or `ART` when it is missing.
fn art_folder(state: &DeviceState) -> io::Result<RelPath> {
    let name = find_entry(&state.top, ART_FOLDER)
        .filter(|e| e.is_dir)
        .map_or(ART_FOLDER, |e| e.name.as_str());
    rel(name)
}

/// The lower-case key and suffix of a picture in the art folder, in any
/// format OPL reads: 1.1.0 also takes `.jpg` and `.bmp`.
fn art_key(entry: &DirEntry) -> Option<(String, String)> {
    if entry.is_dir {
        return None;
    }
    let art = parse_art_file_name(&entry.name)?;
    matches!(
        art.extension.to_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "bmp"
    )
    .then(|| (art.key.to_lowercase(), art.suffix.to_lowercase()))
}

fn wanted(startup: &str, kind: ArtKind) -> (String, String) {
    (startup.to_lowercase(), kind.suffix().to_lowercase())
}

/// Files of `startup` and `kind` in the art folder, any extension.
fn existing_art(entries: &[DirEntry], startup: &str, kind: ArtKind) -> Vec<DirEntry> {
    let wanted = wanted(startup, kind);
    entries
        .iter()
        .filter(|e| art_key(e).as_ref() == Some(&wanted))
        .cloned()
        .collect()
}

/// The art folder of a device, read once per plan.
pub(crate) struct ArtFolder {
    pub folder: RelPath,
    /// Its pictures by [`art_key`], in listing order: a picture set writes
    /// up to eight kinds for every game, so names are parsed once.
    existing: HashMap<(String, String), Vec<DirEntry>>,
    /// `.yaoplm/backup/<id>/ART`, for the files a write replaces.
    pub backup: RelPath,
}

impl ArtFolder {
    pub(crate) fn read(fs: &dyn DeviceFs, state: &DeviceState) -> io::Result<Self> {
        let folder = art_folder(state)?;
        let mut existing: HashMap<_, Vec<DirEntry>> = HashMap::new();
        for entry in list_or_empty(fs, &folder)? {
            if let Some(key) = art_key(&entry) {
                existing.entry(key).or_default().push(entry);
            }
        }
        let backup = rel(&format!(
            "{BACKUP_DIR}/{}/{}",
            journal::new_id(),
            folder.as_str()
        ))?;
        Ok(Self {
            folder,
            existing,
            backup,
        })
    }

    /// The transaction writing `item`, and whether it moves an existing
    /// file to the backup folder first. Without `overwrite` an existing
    /// file skips the item, like a bad name or an unreadable source.
    pub(crate) fn write(
        &self,
        plan: &mut Plan,
        sources: &dyn SourceFs,
        item: &ArtItem,
        overwrite: bool,
    ) -> io::Result<Result<(Transaction, bool), Skipped>> {
        let name = art_file_name(&item.startup, item.kind);
        let target = match join(&self.folder, &name) {
            Ok(path) => path,
            Err(err) => {
                return Ok(Err(Skipped {
                    path: item.file.clone(),
                    reason: Notice::warning(
                        Phrase::notice("badName").with("reason", reasons::bad_name(&err)),
                    ),
                }));
            }
        };
        let meta = match sources.metadata(&item.file) {
            Ok(meta) if !meta.is_dir => meta,
            _ => {
                return Ok(Err(Skipped {
                    path: item.file.clone(),
                    reason: Notice::warning(Phrase::notice("artUnreadable")),
                }));
            }
        };
        let existing = self
            .existing
            .get(&wanted(&item.startup, item.kind))
            .map_or(&[][..], Vec::as_slice);
        if !existing.is_empty() && !overwrite {
            return Ok(Err(Skipped {
                path: item.file.clone(),
                reason: Notice::info(
                    Phrase::notice("artExists")
                        .with("startup", &item.startup)
                        .with("kind", kind_name(item.kind)),
                ),
            }));
        }
        plan.sources.push(Source {
            path: item.file.clone(),
            kind: SourceKind::Raw,
            len: meta.len,
            files: vec![SourceFile {
                path: item.file.clone(),
                len: meta.len,
                modified: meta.modified,
            }],
            on_device: false,
            check: None,
        });
        let source = plan.sources.len() - 1;
        let mut tx = Transaction::titled(
            Phrase::title("artFor")
                .with("kind", kind_name(item.kind))
                .with("title", &item.title),
        );
        tx.game = Some(GameInfo {
            title: item.title.clone(),
            startup: item.startup.clone(),
            media: item.media,
            format: item.format.clone(),
        });
        for old in existing {
            tx.push(Step::Rename {
                from: join(&self.folder, &old.name)?,
                to: join(&self.backup, &old.name)?,
            });
        }
        tx.push(Step::WriteFile {
            path: target,
            source,
            offset: 0,
            len: meta.len,
        });
        if !existing.is_empty() {
            tx.notices.push(Notice::info(
                Phrase::notice("artReplaced").with("folder", self.backup.to_string()),
            ));
        }
        tx.notices.push(Notice::info(
            Phrase::notice("artFrom")
                .with("source", &item.source)
                .with("origin", &item.origin),
        ));
        Ok(Ok((tx, !existing.is_empty())))
    }
}

/// Lower-case keys of the files of `kind` in the art folder, in any of the
/// formats [`plan_set_art`] would refuse to overwrite.
pub fn present_art(fs: &dyn DeviceFs, kind: ArtKind) -> io::Result<HashSet<String>> {
    Ok(present_art_kinds(fs, &[kind])?
        .pop()
        .map(|(_, keys)| keys)
        .unwrap_or_default())
}

/// [`present_art`] for several kinds, in their order, listing the art folder
/// once: on a stick with thousands of pictures that is one slow listing.
pub fn present_art_kinds(
    fs: &dyn DeviceFs,
    kinds: &[ArtKind],
) -> io::Result<Vec<(ArtKind, HashSet<String>)>> {
    let state = DeviceState::read(fs)?;
    let entries = list_or_empty(fs, &art_folder(&state)?)?;
    let files: Vec<(String, String)> = entries.iter().filter_map(art_key).collect();
    Ok(kinds
        .iter()
        .map(|&kind| {
            let suffix = kind.suffix().to_lowercase();
            let keys = files
                .iter()
                .filter(|(_, s)| *s == suffix)
                .map(|(key, _)| key.clone())
                .collect();
            (kind, keys)
        })
        .collect())
}

/// `CreateDir` steps for `.yaoplm/backup/<id>/ART`, outermost first.
pub(crate) fn backup_dirs(backup: &RelPath) -> io::Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut parts = Vec::new();
    let mut at = Some(backup.clone());
    while let Some(dir) = at.filter(|d| !d.is_root()) {
        parts.push(dir.clone());
        at = dir.parent();
    }
    for dir in parts.into_iter().rev() {
        steps.push(Step::CreateDir { path: dir });
    }
    Ok(steps)
}

/// Plans writing `items` to the device. An existing cover is skipped unless
/// `overwrite`, in which case it is moved to the backup folder first.
pub fn plan_set_art(
    fs: &dyn DeviceFs,
    sources: &dyn SourceFs,
    items: &[ArtItem],
    overwrite: bool,
) -> io::Result<Plan> {
    let state = DeviceState::read(fs)?;
    let mut plan = Plan::new(
        PlanKind::Art,
        "Write covers",
        state.info.fingerprint.clone(),
        state.info.free_bytes,
    );
    plan.notices.extend(state.unfinished_notice());
    let art = ArtFolder::read(fs, &state)?;
    let mut need_backup = false;
    let mut seen = HashSet::new();
    let mut transactions = Vec::new();

    for item in items {
        if !seen.insert((item.startup.to_lowercase(), item.kind)) {
            continue;
        }
        match art.write(&mut plan, sources, item, overwrite)? {
            Ok((tx, replaced)) => {
                need_backup |= replaced;
                transactions.push(tx);
            }
            Err(skipped) => plan.skipped.push(skipped),
        }
    }

    let mut prepare = Transaction::titled(Phrase::title("prepareFolders"));
    if !transactions.is_empty() && !state.has_folder(art.folder.as_str()) {
        prepare.push(Step::CreateDir {
            path: art.folder.clone(),
        });
    }
    if need_backup {
        for step in backup_dirs(&art.backup)? {
            prepare.push(step);
        }
    }
    if !prepare.steps.is_empty() {
        plan.transactions.push(prepare);
    }
    let count = transactions.len();
    plan.transactions.extend(transactions);
    plan.set_title(if count == 0 {
        Phrase::title("noArtToWrite")
    } else if items.iter().all(|i| i.kind == ArtKind::Cov) {
        Phrase::title("writeCovers").count(count)
    } else {
        Phrase::title("writeArt").count(count)
    });
    if count == 0 && plan.notices.iter().all(|n| n.code != "unfinishedOperation") {
        plan.notices
            .push(Notice::error(Phrase::notice("nothingToWrite")));
    }
    let needed = plan.write_bytes();
    if count > 0 && needed > state.info.free_bytes {
        plan.notices.push(Notice::error(
            Phrase::notice("noSpace")
                .with("needed", needed)
                .with("free", state.info.free_bytes),
        ));
    }
    Ok(plan)
}

/// Plans removing every `<startup>_<KIND>.*` file, each copied to the
/// backup folder first. Executing it needs explicit confirmation.
pub fn plan_delete_art(fs: &dyn DeviceFs, startup: &str, kind: ArtKind) -> Result<Plan, OpError> {
    let state = DeviceState::read(fs)?;
    let folder = art_folder(&state)?;
    let entries = list_or_empty(fs, &folder)?;
    let existing = existing_art(&entries, startup, kind);
    if existing.is_empty() {
        return Err(OpError::NotFound(
            Phrase::error("noArt")
                .with("startup", startup)
                .with("kind", kind_name(kind)),
        ));
    }
    let backup = rel(&format!(
        "{BACKUP_DIR}/{}/{}",
        journal::new_id(),
        folder.as_str()
    ))?;
    let title = Phrase::title("deleteArt")
        .with("kind", kind_name(kind))
        .with("startup", startup);
    let mut plan = Plan::titled(
        PlanKind::Art,
        title.clone(),
        state.info.fingerprint.clone(),
        state.info.free_bytes,
    );
    plan.notices.extend(state.unfinished_notice());
    let mut tx = Transaction::titled(title);
    for step in backup_dirs(&backup)? {
        tx.push(step);
    }
    for old in &existing {
        tx.push(Step::CopyFile {
            from: join(&folder, &old.name)?,
            to: join(&backup, &old.name)?,
        });
    }
    for old in &existing {
        tx.then(Step::Remove {
            path: join(&folder, &old.name)?,
            len: old.len,
            what: Removal::UserData,
        });
    }
    tx.notices.push(Notice::info(
        Phrase::notice("coverBackedUp").with("folder", backup.to_string()),
    ));
    plan.transactions.push(tx);
    Ok(plan)
}
