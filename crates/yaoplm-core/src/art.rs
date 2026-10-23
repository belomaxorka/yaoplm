//! Plans that put cover art on the device or take it off (ART-1, ART-3,
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
//! One transaction per game: a failure on one cover leaves the others in
//! place, and the journal is small because the bytes stay on the host.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, DirEntry, find_entry, list_or_empty};
use yaoplm_device::source::SourceFs;
use yaoplm_formats::Media;
use yaoplm_formats::art::{ART_FOLDER, ArtKind, art_file_name, parse_art_file_name};

use crate::device::{DeviceState, join, rel};
use crate::journal::{self, BACKUP_DIR};
use crate::ops::OpError;
use crate::phrase::Phrase;
use crate::plan::{
    GameInfo, Notice, Plan, PlanKind, Removal, Skipped, Source, SourceFile, SourceKind, Step,
    Transaction,
};

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
    pub origin: String,
}

/// The art folder as named on the device, or `ART` when it is missing.
fn art_folder(state: &DeviceState) -> io::Result<RelPath> {
    let name = find_entry(&state.top, ART_FOLDER)
        .filter(|e| e.is_dir)
        .map_or(ART_FOLDER, |e| e.name.as_str());
    rel(name)
}

/// Files of `startup` and `kind` in the art folder, any extension: OPL
/// 1.1.0 also reads `.jpg` and `.bmp`.
fn existing_art(entries: &[DirEntry], startup: &str, kind: ArtKind) -> Vec<DirEntry> {
    let (id, suffix) = (startup.to_lowercase(), kind.suffix().to_lowercase());
    entries
        .iter()
        .filter(|e| !e.is_dir)
        .filter(|e| {
            parse_art_file_name(&e.name).is_some_and(|a| {
                a.key.to_lowercase() == id
                    && a.suffix.to_lowercase() == suffix
                    && matches!(
                        a.extension.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "bmp"
                    )
            })
        })
        .cloned()
        .collect()
}

/// `CreateDir` steps for `.yaoplm/backup/<id>/ART`, outermost first.
fn backup_dirs(backup: &RelPath) -> io::Result<Vec<Step>> {
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
    let folder = art_folder(&state)?;
    let entries = list_or_empty(fs, &folder)?;
    let backup = rel(&format!(
        "{BACKUP_DIR}/{}/{}",
        journal::new_id(),
        folder.as_str()
    ))?;
    let mut need_backup = false;
    let mut seen = HashSet::new();
    let mut transactions = Vec::new();

    for item in items {
        if !seen.insert((item.startup.to_lowercase(), item.kind)) {
            continue;
        }
        let name = art_file_name(&item.startup, item.kind);
        let target = match join(&folder, &name) {
            Ok(path) => path,
            Err(err) => {
                plan.skipped.push(Skipped {
                    path: item.file.clone(),
                    reason: Notice::warning("badName", format!("{name}: {err}")),
                });
                continue;
            }
        };
        let meta = match sources.metadata(&item.file) {
            Ok(meta) if !meta.is_dir => meta,
            _ => {
                plan.skipped.push(Skipped {
                    path: item.file.clone(),
                    reason: Notice::warning("coverUnreadable", "the cover file cannot be read"),
                });
                continue;
            }
        };
        let existing = existing_art(&entries, &item.startup, item.kind);
        if !existing.is_empty() && !overwrite {
            plan.skipped.push(Skipped {
                path: item.file.clone(),
                reason: Notice::info(
                    "coverExists",
                    format!("{} already has a {} cover", item.startup, item.kind),
                ),
            });
            continue;
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
        });
        let source = plan.sources.len() - 1;
        let mut tx = Transaction::titled(
            Phrase::title("coverFor")
                .with("kind", item.kind.to_string())
                .with("title", &item.title),
        );
        tx.game = Some(GameInfo {
            title: item.title.clone(),
            startup: item.startup.clone(),
            media: item.media,
            format: item.format.clone(),
        });
        for old in &existing {
            need_backup = true;
            tx.push(Step::Rename {
                from: join(&folder, &old.name)?,
                to: join(&backup, &old.name)?,
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
                "coverReplaced",
                format!("the current cover moves to {backup} before the new one is written"),
            ));
        }
        tx.notices.push(Notice::info(
            "coverFrom",
            format!("cover from {}", item.origin),
        ));
        transactions.push(tx);
    }

    let mut prepare = Transaction::titled(Phrase::title("prepareFolders"));
    if !transactions.is_empty() && !state.has_folder(folder.as_str()) {
        prepare.push(Step::CreateDir {
            path: folder.clone(),
        });
    }
    if need_backup {
        for step in backup_dirs(&backup)? {
            prepare.push(step);
        }
    }
    if !prepare.steps.is_empty() {
        plan.transactions.push(prepare);
    }
    let count = transactions.len();
    plan.transactions.extend(transactions);
    plan.set_title(if count == 0 {
        Phrase::title("noCoverToWrite")
    } else {
        Phrase::title("writeCovers").count(count)
    });
    if count == 0 && plan.notices.iter().all(|n| n.code != "unfinishedOperation") {
        plan.notices
            .push(Notice::error("nothingToWrite", "no cover to write"));
    }
    let needed = plan.write_bytes();
    if count > 0 && needed > state.info.free_bytes {
        plan.notices.push(Notice::error(
            "noSpace",
            format!(
                "not enough free space: {needed} bytes needed, {} available",
                state.info.free_bytes
            ),
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
            Phrase::error("noCover")
                .with("startup", startup)
                .with("kind", kind.to_string()),
        ));
    }
    let backup = rel(&format!(
        "{BACKUP_DIR}/{}/{}",
        journal::new_id(),
        folder.as_str()
    ))?;
    let title = Phrase::title("deleteCover")
        .with("kind", kind.to_string())
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
        "coverBackedUp",
        format!("a copy stays in {backup}"),
    ));
    plan.transactions.push(tx);
    Ok(plan)
}
