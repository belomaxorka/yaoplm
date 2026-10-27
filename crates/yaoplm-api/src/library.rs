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
use crate::phrase::Phrase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum LoaderProfile {
    #[default]
    Opl120Beta,
    Opl110,
    RiptOpl,
}

impl From<core::LoaderProfile> for LoaderProfile {
    fn from(profile: core::LoaderProfile) -> Self {
        match profile {
            core::LoaderProfile::Opl120Beta => Self::Opl120Beta,
            core::LoaderProfile::Opl110 => Self::Opl110,
            core::LoaderProfile::RiptOpl => Self::RiptOpl,
        }
    }
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

impl From<Media> for core::Media {
    fn from(media: Media) -> Self {
        match media {
            Media::CD => Self::Cd,
            Media::DVD => Self::Dvd,
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

/// A warning or skip reason: `code` is stable, `message` is English and
/// `phrase` says it in the front end's language (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct Notice {
    pub code: String,
    pub message: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<Phrase>,
}

impl Notice {
    fn said(code: &str, phrase: &yaoplm_core::Phrase) -> Self {
        Self {
            code: code.to_owned(),
            message: phrase.english(),
            phrase: Some(Phrase::from(phrase)),
        }
    }
}

impl From<&core::Warning> for Notice {
    fn from(warning: &core::Warning) -> Self {
        Self::said(warning.code(), &warning.phrase())
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
    pub size: u64,
    /// Milliseconds since the Unix epoch, for cache busting.
    pub modified: Option<u64>,
}

impl From<&core::ArtFile> for Art {
    fn from(art: &core::ArtFile) -> Self {
        Self {
            path: display_path(&art.path),
            kind: art.suffix.clone(),
            width: art.dimensions.map(|d| d.0),
            height: art.dimensions.map(|d| d.1),
            size: art.size,
            modified: art.modified,
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
    /// Position in `ul.cfg`, for `ul` games (see `GameRef`).
    pub ul_index: Option<u32>,
    pub size: u64,
    /// The ID in `SYSTEM.CNF`, when it was read.
    pub image_startup: Option<String>,
    /// DVD-9: the sector where OPL starts layer 1.
    pub layer1_start: Option<u32>,
    pub cfg: Option<GameCfg>,
    pub art: Vec<Art>,
    pub cheats: Option<u32>,
    pub warnings: Vec<Notice>,
    /// The title the bundled database gives this ID (META-1).
    pub known_title: Option<String>,
    /// What the bundled game info table says about the ID (META-3).
    pub info: Option<GameInfoView>,
    /// The last verification of this image, when its hashes are cached and
    /// it has not changed since (GAME-5). Scans never hash.
    pub dump: Option<DumpCheck>,
    /// How the game lies on a BDM device (IMP-7b); `null` off BDM.
    pub fragments: Option<Fragments>,
}

/// Runs of clusters a game's files occupy on a FAT32 or exFAT device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fragments {
    Known {
        total: u32,
        /// One count per file, in part order.
        files: Vec<u32>,
        /// OPL's limit for the whole game under the profile; `null` when
        /// every file must be one piece (OPL 1.1.0).
        limit: Option<u32>,
    },
    /// The OS does not say where files lie (Linux without root).
    Unknown,
}

impl Fragments {
    pub fn from_core(fragments: &core::Fragments, profile: core::LoaderProfile) -> Self {
        match fragments {
            core::Fragments::Known(files) => Self::Known {
                total: fragments.total().unwrap_or(u32::MAX),
                files: files.clone(),
                limit: match profile.fragment_limit() {
                    yaoplm_formats::FragmentLimit::Total(limit) => Some(limit),
                    yaoplm_formats::FragmentLimit::OnePerFile => None,
                },
            },
            core::Fragments::Unknown => Self::Unknown,
        }
    }
}

/// A game's standing against Redump, from the hash cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DumpCheck {
    pub status: crate::verify::VerifyStatus,
    pub reason: Option<crate::verify::UnknownReason>,
    /// The Redump name of the matching dump.
    pub redump: Option<String>,
    /// The verdict compares the raw CD track the game was added from, not
    /// the image.
    #[serde(default)]
    pub by_track: bool,
}

impl Game {
    fn from_core(game: &core::Game, root: &std::path::Path, profile: core::LoaderProfile) -> Self {
        let ul_index = match &game.location {
            core::GameLocation::Ul { index, .. } => u32::try_from(*index).ok(),
            core::GameLocation::Image { .. } => None,
        };
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
            ul_index,
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
            known_title: core::titles::known_title(&game.startup),
            info: game_info_view(&game.startup),
            dump: None,
            fragments: game
                .fragments
                .as_ref()
                .map(|f| Fragments::from_core(f, profile)),
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
                .map(|g| Game::from_core(g, &library.root, profile.into()))
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
                    reason: Notice::said(s.reason.code(), &s.reason.phrase()),
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

/// Where a game-info value comes from (META-3, ADR-0050).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum InfoSource {
    /// libretro-database's developer DAT: PSXDataCenter's facts for the
    /// serial itself.
    Libretro,
    /// The Wikidata item of the game, which covers every regional release.
    Wikidata,
}

impl From<core::game_info::InfoSource> for InfoSource {
    fn from(source: core::game_info::InfoSource) -> Self {
        match source {
            core::game_info::InfoSource::Libretro => Self::Libretro,
            core::game_info::InfoSource::Wikidata => Self::Wikidata,
        }
    }
}

/// The genre vocabulary (ADR-0050); the values are the i18n keys
/// `genre.<value>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "kebab-case")]
pub enum Genre {
    Action,
    ActionAdventure,
    Adventure,
    Platform,
    Shooter,
    Fighting,
    BeatEmUp,
    Racing,
    Sports,
    Rpg,
    Strategy,
    Simulation,
    Puzzle,
    Music,
    Party,
    Board,
    Card,
    Casino,
    Mahjong,
    Quiz,
    Horror,
    VisualNovel,
    Educational,
    Pinball,
    Other,
}

impl From<core::game_info::Genre> for Genre {
    fn from(genre: core::game_info::Genre) -> Self {
        use core::game_info::Genre as G;
        match genre {
            G::Action => Self::Action,
            G::ActionAdventure => Self::ActionAdventure,
            G::Adventure => Self::Adventure,
            G::Platform => Self::Platform,
            G::Shooter => Self::Shooter,
            G::Fighting => Self::Fighting,
            G::BeatEmUp => Self::BeatEmUp,
            G::Racing => Self::Racing,
            G::Sports => Self::Sports,
            G::Rpg => Self::Rpg,
            G::Strategy => Self::Strategy,
            G::Simulation => Self::Simulation,
            G::Puzzle => Self::Puzzle,
            G::Music => Self::Music,
            G::Party => Self::Party,
            G::Board => Self::Board,
            G::Card => Self::Card,
            G::Casino => Self::Casino,
            G::Mahjong => Self::Mahjong,
            G::Quiz => Self::Quiz,
            G::Horror => Self::Horror,
            G::VisualNovel => Self::VisualNovel,
            G::Educational => Self::Educational,
            G::Pinball => Self::Pinball,
            G::Other => Self::Other,
        }
    }
}

impl Genre {
    /// The code in the data file and the i18n key suffix.
    pub fn code(self) -> &'static str {
        core::game_info::Genre::ALL
            .into_iter()
            .find(|g| Genre::from(*g) == self)
            .map_or("other", core::game_info::Genre::code)
    }
}

/// A text value with its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TextFact {
    pub value: String,
    pub source: InfoSource,
}

/// A number with its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct NumberFact {
    pub value: u32,
    pub source: InfoSource,
}

/// Genres with their source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GenresFact {
    pub value: Vec<Genre>,
    pub source: InfoSource,
}

/// The Wikidata item of the game.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct WikidataLink {
    /// `Q149106`.
    pub qid: String,
    pub url: String,
    /// `true` when the item's PlayStation ID is this serial; otherwise the
    /// item was matched by title.
    pub by_id: bool,
    pub label_en: Option<String>,
    pub label_ru: Option<String>,
    /// Wikipedia article titles, when the game has an article.
    pub article_en: Option<String>,
    pub article_ru: Option<String>,
}

/// What the bundled game info table says about a game (META-3): each
/// value with its source, the serial's own facts before the title's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct GameInfoView {
    /// `SLUS-20062`.
    pub serial: String,
    pub developer: Option<TextFact>,
    pub publisher: Option<TextFact>,
    /// `2001-10-23`, `2001-10` or `2001`.
    pub released: Option<TextFact>,
    pub players: Option<NumberFact>,
    pub genres: Option<GenresFact>,
    pub wikidata: Option<WikidataLink>,
}

impl From<&core::game_info::GameInfo> for GameInfoView {
    fn from(info: &core::game_info::GameInfo) -> Self {
        let text = |fact: Option<core::game_info::Sourced<String>>| {
            fact.map(|f| TextFact {
                value: f.value,
                source: f.source.into(),
            })
        };
        Self {
            serial: info.serial.clone(),
            developer: text(info.developer()),
            publisher: text(info.publisher()),
            released: info.released().map(|f| TextFact {
                value: f.value.iso(),
                source: f.source.into(),
            }),
            players: info.players().map(|f| NumberFact {
                value: f.value,
                source: f.source.into(),
            }),
            genres: info.genres().map(|f| GenresFact {
                value: f.value.into_iter().map(Genre::from).collect(),
                source: f.source.into(),
            }),
            wikidata: info.wikidata.as_ref().map(|w| WikidataLink {
                qid: w.qid.clone(),
                url: w.url(),
                by_id: w.matched_by == core::game_info::MatchedBy::Id,
                label_en: w.label_en.clone(),
                label_ru: w.label_ru.clone(),
                article_en: w.article_en.clone(),
                article_ru: w.article_ru.clone(),
            }),
        }
    }
}

/// The bundled game info for `id`, if the table knows it.
pub fn game_info_view(id: &str) -> Option<GameInfoView> {
    core::game_info::known_info(id).map(GameInfoView::from)
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
    /// The title the bundled database gives the ID (META-1).
    pub known_title: Option<String>,
    /// What the bundled game info table says about the ID (META-3).
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<GameInfoView>,
    /// How the file lies on a FAT32 or exFAT device (IMP-7b); `null` when
    /// the file is elsewhere.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragments: Option<Fragments>,
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
            known_title: info
                .startup
                .as_ref()
                .ok()
                .and_then(|id| core::titles::known_title(id)),
            info: info.startup.as_ref().ok().and_then(|id| game_info_view(id)),
            fragments: None,
        }
    }
}
