//! What changes for the games when the loader is replaced by a build with
//! other rules (SPEC LDR-7, §5.10.3, ADR-0046). The library is scanned with
//! the new build's profile, the same rules the library screen uses
//! (LIB-4, ADR-0037), and what that profile cannot use is sorted by what
//! the replace plan can do about it:
//! - an exFAT device under OPL 1.1.0: nothing; replacing is refused;
//! - ZSO under OPL 1.1.0: convert to ISO (`ul` on FAT32 above 4 GiB) in the
//!   same plan;
//! - a game in several pieces, or one whose pieces the OS cannot count,
//!   under OPL 1.1.0: only the list and the README's advice (copy, format,
//!   add back); YAOPLM does not defragment;
//! - JPG and BMP art under the betas (PNG only since `c5a12a0`, research/02):
//!   re-encode to PNG in the same plan, the old files kept as a backup.
//!
//! Builds with the same profile (OPL 1.2.0 beta and YAOPL) change nothing.

use std::path::{Path, PathBuf};

use yaoplm_device::FsKind;
use yaoplm_formats::LoaderProfile;
use yaoplm_i18n::Phrase;

use crate::model::{Fragments, GameLocation, Library, Warning};

/// A game the new build would not show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactGame {
    pub startup: String,
    pub title: String,
    /// The image, relative to the device root; `None` for a `ul` game.
    pub path: Option<PathBuf>,
    pub size: u64,
    /// Pieces of its files, when known.
    pub fragments: Option<u32>,
}

/// A picture the new build would not load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactArt {
    pub startup: String,
    pub title: String,
    /// `COV`, `ICO`, …
    pub suffix: String,
    /// Relative to the device root: `ART/SLUS_200.62_COV.jpg`.
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impact {
    pub from: LoaderProfile,
    pub to: LoaderProfile,
    /// The new build cannot use the device at all: no replace plan.
    pub blocked: Option<Phrase>,
    /// Convertible to ISO in the same plan.
    pub zso: Vec<ImpactGame>,
    /// In several pieces: only a list and advice.
    pub fragmented: Vec<ImpactGame>,
    /// The OS cannot say how many pieces.
    pub unknown_fragments: Vec<ImpactGame>,
    /// Re-encodable to PNG in the same plan.
    pub art: Vec<ImpactArt>,
}

impl Impact {
    /// Nothing changes for the games.
    pub fn is_empty(&self) -> bool {
        self.blocked.is_none()
            && self.zso.is_empty()
            && self.fragmented.is_empty()
            && self.unknown_fragments.is_empty()
            && self.art.is_empty()
    }
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// What replacing a `from` build by a `to` build changes, from `library`
/// scanned with the `to` profile on a device of `kind`.
pub fn impact(library: &Library, kind: FsKind, from: LoaderProfile, to: LoaderProfile) -> Impact {
    let mut impact = Impact {
        from,
        to,
        blocked: None,
        zso: Vec::new(),
        fragmented: Vec::new(),
        unknown_fragments: Vec::new(),
        art: Vec::new(),
    };
    if from == to {
        return impact;
    }
    if to == LoaderProfile::Opl110 && kind == FsKind::ExFat {
        impact.blocked = Some(Phrase::notice("replaceExfatOpl110"));
        return impact;
    }
    for game in &library.games {
        let entry = || ImpactGame {
            startup: game.startup.clone(),
            title: game.title.clone(),
            path: match &game.location {
                GameLocation::Image { path, .. } => Some(relative(&library.root, path)),
                GameLocation::Ul { .. } => None,
            },
            size: game.size,
            fragments: game.fragments.as_ref().and_then(Fragments::total),
        };
        let has = |code: &str| game.warnings.iter().any(|w| w.code() == code);
        if has("zsoUnsupportedByProfile") {
            impact.zso.push(entry());
        } else if has("gameFragmentedOpl110") {
            impact.fragmented.push(entry());
        } else if to == LoaderProfile::Opl110 && game.fragments == Some(Fragments::Unknown) {
            impact.unknown_fragments.push(entry());
        }
        let unloaded: Vec<&str> = game
            .warnings
            .iter()
            .filter_map(|w| match w {
                Warning::ArtNotLoaded(file) => Some(file.as_str()),
                _ => None,
            })
            .collect();
        for art in &game.art {
            let name = art
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if unloaded.iter().any(|f| f.eq_ignore_ascii_case(&name)) {
                impact.art.push(ImpactArt {
                    startup: game.startup.clone(),
                    title: game.title.clone(),
                    suffix: art.suffix.clone(),
                    path: relative(&library.root, &art.path),
                    size: art.size,
                });
            }
        }
    }
    impact
}
