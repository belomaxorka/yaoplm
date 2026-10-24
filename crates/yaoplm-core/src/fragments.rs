//! Fragment checks of games on BDM devices (IMP-7b, LIB-4).
//!
//! When OPL starts a game from USB, MX4SIO, iLink or a BDM HDD,
//! `bdmLaunchGame()` (`src/bdmsupport.c`, ps2homebrew/Open-PS2-Loader@
//! b3a21997) asks the driver for the runs of clusters of every part and
//! copies them into one table of `BDM_MAX_FRAGS` (64) entries. It checks
//! the limit per part only, so a game over it in total is not refused
//! cleanly; YAOPLM holds the total to it. OPL 1.1.0 wants every file in
//! one run, and a VMC must be one run on any build (research/02 §4.7, §9;
//! research/06 §1.11). SMB and MMCE cards have no such limit: the check
//! runs only on FAT32 and exFAT volumes OPL reads over BDM, never in SMB
//! mode (research/18 §4, ADR-0043).
//!
//! The counts come from the OS ([`DeviceFs::fragments`]); where it cannot
//! tell, a game is [`Fragments::Unknown`] and gets no warning.

use std::io;
use std::path::{Component, Path};

use yaoplm_device::fs::{find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, FsKind, RelPath};
use yaoplm_formats::{FragmentLimit, LoaderProfile, vmc};

use crate::model::{Fragments, Game, GameLocation, Library, Warning};
use crate::plan::Notice;

/// Whether OPL reads this device over BDM, where the limits apply.
pub fn applies(fs: &dyn DeviceFs) -> bool {
    fs.info()
        .is_ok_and(|info| !info.smb && matches!(info.kind, FsKind::Fat32 | FsKind::ExFat))
}

/// Runs of clusters of each file; unknown as soon as one file's count is.
pub fn count(fs: &dyn DeviceFs, files: &[RelPath]) -> io::Result<Fragments> {
    let mut runs = Vec::with_capacity(files.len());
    for file in files {
        match fs.fragments(file)? {
            Some(n) => runs.push(n),
            None => return Ok(Fragments::Unknown),
        }
    }
    Ok(Fragments::Known(runs))
}

/// The warning OPL's rule calls for, if any.
pub fn warning(fragments: &Fragments, profile: LoaderProfile) -> Option<Warning> {
    let Fragments::Known(runs) = fragments else {
        return None;
    };
    let limit = profile.fragment_limit();
    if limit.allows(runs) {
        return None;
    }
    let total = fragments.total().unwrap_or(u32::MAX);
    Some(match limit {
        FragmentLimit::Total(limit) => Warning::Fragmented {
            fragments: total,
            limit,
        },
        FragmentLimit::OnePerFile => Warning::FragmentedForOpl110 { fragments: total },
    })
}

/// The same check for a game an operation just wrote: a notice for the
/// report, with the advice in its text.
pub fn written_notice(
    fs: &dyn DeviceFs,
    files: &[RelPath],
    profile: LoaderProfile,
) -> Option<Notice> {
    let fragments = count(fs, files).ok()?;
    warning(&fragments, profile).map(|w| Notice::warning(w.phrase()))
}

/// `path` under `root` as a device path, when it is inside it.
fn relative(root: &Path, path: &Path) -> Option<RelPath> {
    let rest = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rest
        .components()
        .map(|c| match c {
            Component::Normal(name) => name.to_str(),
            _ => None,
        })
        .collect();
    RelPath::new(&parts?.join("/")).ok()
}

fn game_files(root: &Path, game: &Game) -> Option<Vec<RelPath>> {
    match &game.location {
        GameLocation::Image { path, .. } => Some(vec![relative(root, path)?]),
        GameLocation::Ul { parts, .. } => parts
            .iter()
            .map(|part| relative(root, part.path.as_deref()?))
            .collect(),
    }
}

/// Counts the runs of every game and of the VMCs their CFGs name, and adds
/// the LIB-4 warnings. `fs` is rooted at `library.root`. Does nothing off
/// BDM; stops early when `cancelled` says so.
pub fn check_library(
    fs: &dyn DeviceFs,
    library: &mut Library,
    profile: LoaderProfile,
    cancelled: &dyn Fn() -> bool,
) {
    if !applies(fs) {
        return;
    }
    let vmc_dir = RelPath::new(vmc::VMC_FOLDER).ok();
    let vmcs = vmc_dir
        .as_ref()
        .and_then(|dir| list_or_empty(fs, dir).ok())
        .unwrap_or_default();
    for game in &mut library.games {
        if cancelled() {
            return;
        }
        // A game with a part missing does not start anyway.
        let Some(files) = game_files(&library.root, game) else {
            continue;
        };
        let Ok(fragments) = count(fs, &files) else {
            continue;
        };
        game.warnings.extend(warning(&fragments, profile));
        game.fragments = Some(fragments);

        let names = game
            .cfg
            .as_ref()
            .map(|cfg| cfg.vmc.clone())
            .unwrap_or_default();
        for (slot, name) in names.iter().enumerate() {
            let (Some(name), Some(dir)) = (name, &vmc_dir) else {
                continue;
            };
            let Some(entry) = find_entry(&vmcs, &vmc::file_name(name)) else {
                continue;
            };
            let Ok(path) = dir.join(&entry.name) else {
                continue;
            };
            if let Ok(Some(runs)) = fs.fragments(&path)
                && runs > 1
            {
                game.warnings.push(Warning::VmcFragmented {
                    slot: slot as u8,
                    name: name.clone(),
                    fragments: runs,
                });
            }
        }
    }
}
