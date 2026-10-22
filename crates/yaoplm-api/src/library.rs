//! Wire types for library scans and image details (LIB-2, LIB-4, CLI-1).
//!
//! Mirrors `yaoplm_core`'s model in a front-end friendly shape: camelCase
//! fields, paths as display strings, warnings as a stable `code` plus an
//! English `message` (UI translations key off the code).

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_core as core;
use yaoplm_formats::iso_name::NameFormat as CoreNameFormat;
use yaoplm_formats::iso9660::Layers;

use crate::paths::display_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum LoaderProfile {
    #[default]
    Opl120Beta,
    Opl110,
    RiptOpl,
}

impl From<LoaderProfile> for core::LoaderProfile {
    fn from(profile: LoaderProfile) -> Self {
        match profile {
            LoaderProfile::Opl120Beta => Self::Opl120Beta,
            LoaderProfile::Opl110 => Self::Opl110,
            LoaderProfile::RiptOpl => Self::RiptOpl,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum Media {
    CD,
    DVD,
}

impl From<core::Media> for Media {
    fn from(media: core::Media) -> Self {
        match media {
            core::Media::Cd => Self::CD,
            core::Media::Dvd => Self::DVD,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum GameFormat {
    ISO,
    ZSO,
    UL,
}

impl From<core::GameFormat> for GameFormat {
    fn from(format: core::GameFormat) -> Self {
        match format {
            core::GameFormat::Iso => Self::ISO,
            core::GameFormat::Zso => Self::ZSO,
            core::GameFormat::Ul => Self::UL,
        }
    }
}

/// Old: `SLUS_200.62.Title.iso`, the ID comes from the name. New:
/// `Title.iso`, OPL reads `SYSTEM.CNF`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum NameFormat {
    Old,
    New,
}

impl From<CoreNameFormat> for NameFormat {
    fn from(format: CoreNameFormat) -> Self {
        match format {
            CoreNameFormat::Old => Self::Old,
            CoreNameFormat::New => Self::New,
        }
    }
}

/// A warning or skip reason: `code` is stable, `message` is English.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct Notice {
    pub code: String,
    pub message: String,
}

impl From<&core::Warning> for Notice {
    fn from(warning: &core::Warning) -> Self {
        Self {
            code: warning.code().to_owned(),
            message: warning.to_string(),
        }
    }
}

fn notices(warnings: &[core::Warning]) -> Vec<Notice> {
    warnings.iter().map(Notice::from).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UlPart {
    pub name: String,
    pub present: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GameCfg {
    pub path: String,
    /// False for wOPL's libconfig files, which YAOPLM never rewrites.
    pub opl_syntax: bool,
    pub compatibility: Option<i32>,
    /// `$VMC_0` and `$VMC_1`.
    pub vmc: Vec<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Art {
    pub path: String,
    /// `COV`, `ICO`, ...
    pub kind: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl From<&core::ArtFile> for Art {
    fn from(art: &core::ArtFile) -> Self {
        Self {
            path: display_path(&art.path),
            kind: art.suffix.clone(),
            width: art.dimensions.map(|d| d.0),
            height: art.dimensions.map(|d| d.1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub title: String,
    /// The ID OPL uses for CFG, ART, CHT and VMC names.
    pub startup: String,
    pub media: Media,
    pub format: GameFormat,
    /// The image file, or `ul.cfg` for `ul` games.
    pub path: String,
    pub name_format: Option<NameFormat>,
    /// `ul` chunks; empty for images.
    pub parts: Vec<UlPart>,
    pub size: u64,
    /// The ID in `SYSTEM.CNF`, when it was read.
    pub image_startup: Option<String>,
    /// DVD-9: the sector where OPL starts layer 1.
    pub layer1_start: Option<u32>,
    pub cfg: Option<GameCfg>,
    pub art: Vec<Art>,
    pub cheats: Option<u32>,
    pub warnings: Vec<Notice>,
}

impl Game {
    fn from_core(game: &core::Game, root: &std::path::Path) -> Self {
        let (path, name_format, parts) = match &game.location {
            core::GameLocation::Image {
                path, name_format, ..
            } => (display_path(path), Some((*name_format).into()), Vec::new()),
            core::GameLocation::Ul { parts, .. } => (
                display_path(&root.join("ul.cfg")),
                None,
                parts
                    .iter()
                    .map(|p| UlPart {
                        name: p.name.clone(),
                        present: p.path.is_some(),
                        size: p.size,
                    })
                    .collect(),
            ),
        };
        Self {
            title: game.title.clone(),
            startup: game.startup.clone(),
            media: game.media.into(),
            format: game.format.into(),
            path,
            name_format,
            parts,
            size: game.size,
            image_startup: game.image_startup.clone(),
            layer1_start: match game.layers {
                Some(Layers::Dual { layer1_start }) => Some(layer1_start),
                _ => None,
            },
            cfg: game.cfg.as_ref().map(|cfg| GameCfg {
                path: display_path(&cfg.path),
                opl_syntax: cfg.dialect == yaoplm_formats::cfg::Dialect::Opl,
                compatibility: cfg.compatibility,
                vmc: cfg.vmc.to_vec(),
            }),
            art: game.art.iter().map(Art::from).collect(),
            cheats: game
                .cheats
                .as_ref()
                .map(|c| u32::try_from(c.cheats).unwrap_or(u32::MAX)),
            warnings: notices(&game.warnings),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub folder: String,
    pub title: String,
    pub boot: String,
    pub boot_found: bool,
    pub art: Vec<Art>,
    pub warnings: Vec<Notice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    pub reason: Notice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub name: String,
    pub present: bool,
}

/// A device root as OPL would list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Library {
    pub root: String,
    pub profile: LoaderProfile,
    pub folders: Vec<Folder>,
    pub games: Vec<Game>,
    pub apps: Vec<App>,
    pub skipped: Vec<SkippedFile>,
    pub warnings: Vec<Notice>,
}

impl Library {
    pub fn from_core(library: &core::Library, profile: LoaderProfile) -> Self {
        Self {
            root: display_path(&library.root),
            profile,
            folders: library
                .folders
                .iter()
                .map(|(name, present)| Folder {
                    name: name.clone(),
                    present: *present,
                })
                .collect(),
            games: library
                .games
                .iter()
                .map(|g| Game::from_core(g, &library.root))
                .collect(),
            apps: library
                .apps
                .iter()
                .map(|a| App {
                    folder: a.folder.clone(),
                    title: a.title.clone(),
                    boot: a.boot.clone(),
                    boot_found: a.boot_found,
                    art: a.art.iter().map(Art::from).collect(),
                    warnings: notices(&a.warnings),
                })
                .collect(),
            skipped: library
                .skipped
                .iter()
                .map(|s| SkippedFile {
                    path: display_path(&s.path),
                    reason: Notice {
                        code: s.reason.code().to_owned(),
                        message: s.reason.to_string(),
                    },
                })
                .collect(),
            warnings: notices(&library.warnings),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum Platform {
    PS2,
    PS1,
    Unknown,
}

/// How OPL splits the file name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct FileName {
    pub format: NameFormat,
    pub startup: Option<String>,
    pub title: String,
    pub extension: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ZsoHeader {
    pub total_bytes: u64,
    pub block_size: u32,
    pub version: u8,
    pub align: u8,
}

/// Everything YAOPLM can tell about one disc image (`yaoplm info`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImageDetails {
    pub path: String,
    pub file_size: u64,
    pub format: GameFormat,
    pub zso: Option<ZsoHeader>,
    pub sectors: u64,
    pub volume_id: Option<String>,
    pub volume_sectors: Option<u32>,
    pub layer1_start: Option<u32>,
    pub system_cnf: Option<String>,
    pub boot2: Option<String>,
    pub boot: Option<String>,
    pub ver: Option<String>,
    pub vmode: Option<String>,
    /// The ID OPL shows; `None` when OPL hides the image.
    pub startup: Option<String>,
    /// Why OPL hides the image.
    pub hidden_reason: Option<String>,
    pub platform: Platform,
    pub suggested_media: Media,
    pub file_name: Option<FileName>,
    pub warnings: Vec<Notice>,
}

impl From<&core::ImageInfo> for ImageDetails {
    fn from(info: &core::ImageInfo) -> Self {
        Self {
            path: display_path(&info.path),
            file_size: info.file_size,
            format: info.format.into(),
            zso: info.zso.map(|h| ZsoHeader {
                total_bytes: h.total_bytes,
                block_size: h.block_size,
                version: h.version,
                align: h.align,
            }),
            sectors: info.sectors,
            volume_id: info.volume_id.clone(),
            volume_sectors: info.volume_sectors,
            layer1_start: match info.layers {
                Some(Layers::Dual { layer1_start }) => Some(layer1_start),
                _ => None,
            },
            system_cnf: info.system_cnf.clone(),
            boot2: info.cnf.boot2.clone(),
            boot: info.cnf.boot.clone(),
            ver: info.cnf.ver.clone(),
            vmode: info.cnf.vmode.clone(),
            startup: info.startup.as_ref().ok().cloned(),
            hidden_reason: info.startup.as_ref().err().cloned(),
            platform: match info.platform {
                core::Platform::Ps2 => Platform::PS2,
                core::Platform::Ps1 => Platform::PS1,
                core::Platform::Unknown => Platform::Unknown,
            },
            suggested_media: info.suggested_media.into(),
            file_name: info.file_name.as_ref().map(|n| FileName {
                format: n.format.into(),
                startup: n.startup.clone(),
                title: n.title.clone(),
                extension: n.raw_extension.clone(),
            }),
            warnings: notices(&info.warnings),
        }
    }
}
