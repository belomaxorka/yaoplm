//! What a library looks like to OPL, plus what YAOPLM noticed about it.

use std::fmt;
use std::path::PathBuf;

use yaoplm_formats::Media;
use yaoplm_formats::cfg::Dialect;
use yaoplm_formats::cht::ChtWarning;
use yaoplm_formats::iso_name::NameFormat;
use yaoplm_formats::iso9660::Layers;
use yaoplm_formats::ul_cfg::RecordAnomaly;
use yaoplm_formats::vmc::VmcProblem;
use yaoplm_formats::zso::ZsoProblem;
use yaoplm_i18n::Phrase;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameFormat {
    Iso,
    Zso,
    /// USBExtreme chunks listed in `ul.cfg`.
    Ul,
}

impl fmt::Display for GameFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Iso => "ISO",
            Self::Zso => "ZSO",
            Self::Ul => "UL",
        })
    }
}

/// Where a game's data lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameLocation {
    /// An image in `CD/` or `DVD/`.
    Image {
        path: PathBuf,
        file_name: String,
        name_format: NameFormat,
    },
    /// Record `index` of `ul.cfg` and its chunk files in the root.
    Ul { index: usize, parts: Vec<UlPart> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UlPart {
    /// `ul.%08X.%s.%02x`.
    pub name: String,
    /// `None` when the chunk is missing.
    pub path: Option<PathBuf>,
    pub size: Option<u64>,
}

/// A game as OPL lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// The title OPL shows.
    pub title: String,
    /// The ID OPL uses for CFG, ART, CHT and VMC names.
    pub startup: String,
    pub media: Media,
    pub format: GameFormat,
    pub location: GameLocation,
    /// Bytes on the device (all parts for `ul`).
    pub size: u64,
    /// Read from `SYSTEM.CNF`, when the image was readable.
    pub image_startup: Option<String>,
    /// DVD-9 layer break as OPL would detect it.
    pub layers: Option<Layers>,
    pub cfg: Option<CfgSummary>,
    pub art: Vec<ArtFile>,
    pub cheats: Option<CheatSummary>,
    /// How the game lies on a BDM device; `None` when not checked (a
    /// share, an NTFS folder, or no check asked for).
    pub fragments: Option<Fragments>,
    pub warnings: Vec<Warning>,
}

/// Runs of clusters a game's files occupy (IMP-7b).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fragments {
    /// One count per file, in part order.
    Known(Vec<u32>),
    /// The OS does not say where files lie: Linux without root, or a
    /// macOS driver without a block map.
    Unknown,
}

impl Fragments {
    /// All runs together, when known.
    pub fn total(&self) -> Option<u32> {
        match self {
            Self::Known(runs) => Some(runs.iter().fold(0u32, |sum, &n| sum.saturating_add(n))),
            Self::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfgSummary {
    pub path: PathBuf,
    pub dialect: Dialect,
    pub compatibility: Option<i32>,
    /// `$VMC_0`, `$VMC_1`.
    pub vmc: [Option<String>; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtFile {
    pub path: PathBuf,
    pub suffix: String,
    /// From the PNG header; `None` for other formats or unreadable files.
    pub dimensions: Option<(u32, u32)>,
    pub size: u64,
    /// Milliseconds since the Unix epoch: front ends use it to tell a
    /// replaced cover from a cached one.
    pub modified: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheatSummary {
    pub path: PathBuf,
    pub cheats: usize,
}

/// An app from `APPS/<folder>/title.cfg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub folder: String,
    pub title: String,
    pub boot: String,
    pub boot_found: bool,
    pub art: Vec<ArtFile>,
    pub warnings: Vec<Warning>,
}

/// A file in `CD/`, `DVD/` or `APPS/` that OPL does not list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: SkipReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// A disc-image extension OPL does not read, e.g. `.cso`, `.bin`.
    UnsupportedExtension(String),
    TitleTooLong,
    /// OPL mounts it and finds no usable ID; the phrase says why.
    NoStartup(Phrase),
    Unreadable(String),
    /// `APPS/<folder>` without a usable `title.cfg`; the phrase says why.
    BadApp(Phrase),
}

impl SkipReason {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnsupportedExtension(_) => "unsupportedExtension",
            Self::TitleTooLong => "titleTooLong",
            Self::NoStartup(_) => "noStartup",
            Self::Unreadable(_) => "unreadable",
            Self::BadApp(_) => "badApp",
        }
    }

    /// What front ends show (ADR-0029); `Display` is its English.
    pub fn phrase(&self) -> Phrase {
        let phrase = Phrase::notice(self.code());
        match self {
            Self::UnsupportedExtension(ext) => phrase.with("ext", ext),
            Self::TitleTooLong => phrase,
            Self::NoStartup(why) | Self::BadApp(why) => phrase.with("reason", why.clone()),
            Self::Unreadable(why) => phrase.with("error", why),
        }
    }
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.phrase().english())
    }
}

/// Something that works differently than the user probably expects
/// (LIB-4). Never fatal for the scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Warning {
    /// `games.bin` maps this file name to another ID; OPL shows the cached one.
    StaleCache {
        cached: String,
        image: String,
    },
    /// An old-style name carries a different ID than `SYSTEM.CNF`.
    NameIdMismatch {
        name: String,
        image: String,
    },
    /// OPL lists `.ISO`/`.ZSO` but shows no format label.
    UppercaseExtension,
    /// FatFs maps names to code page 869; other characters get mangled.
    NonAsciiName,
    /// Over 159 bytes: the in-game SMB driver cannot open it.
    SmbNameTooLong(usize),
    /// The extension says one container, the content another.
    ContainerMismatch {
        extension: GameFormat,
        content: GameFormat,
    },
    ZsoUnsupportedByProfile,
    ZsoHeader(ZsoProblem),
    /// Bigger than any PS2 CD; probably belongs in `DVD/`.
    CdTooLarge(u64),
    UlRecord(RecordAnomaly),
    UlPartMissing {
        part: u8,
        name: String,
    },
    /// Every part but the last must be exactly 1 GiB.
    UlPartSize {
        part: u8,
        name: String,
        size: u64,
    },
    UlTooManyPartsForSmb(u8),
    /// wOPL's libconfig CFG: YAOPLM reads nothing from it and never rewrites it.
    CfgLibconfig,
    CfgUnreadable(String),
    CompatibilityBits {
        value: i32,
        allowed: u32,
    },
    VmcMissing {
        slot: u8,
        name: String,
    },
    VmcInvalid {
        slot: u8,
        name: String,
        problem: VmcProblem,
    },
    ArtNotLoaded(String),
    ArtTooLarge {
        file: String,
        width: u32,
        height: u32,
    },
    Cheats(ChtWarning),
    /// Several entries share this ID, and with it CFG, ART, CHT and VMC.
    DuplicateStartup(String),
    UlCfgTrailingBytes(usize),
    AppBootMissing(String),
    /// A YAOPLM operation did not finish; its journal is still on the device.
    UnfinishedOperation(String),
    /// More runs of clusters in total than OPL's BDM table holds (IMP-7b).
    Fragmented {
        fragments: u32,
        limit: u32,
    },
    /// A file of the game in several runs: OPL 1.1.0 refuses it on BDM.
    FragmentedForOpl110 {
        fragments: u32,
    },
    /// The VMC in a slot is not one run: OPL offers the real card instead.
    VmcFragmented {
        slot: u8,
        name: String,
        fragments: u32,
    },
}

impl Warning {
    /// Stable identifier for front ends (i18n keys, filters).
    pub fn code(&self) -> &'static str {
        match self {
            Self::StaleCache { .. } => "staleCache",
            Self::NameIdMismatch { .. } => "nameIdMismatch",
            Self::UppercaseExtension => "uppercaseExtension",
            Self::NonAsciiName => "nonAsciiName",
            Self::SmbNameTooLong(_) => "smbNameTooLong",
            Self::ContainerMismatch { .. } => "containerMismatch",
            Self::ZsoUnsupportedByProfile => "zsoUnsupportedByProfile",
            Self::ZsoHeader(_) => "zsoHeader",
            Self::CdTooLarge(_) => "cdTooLarge",
            Self::UlRecord(_) => "ulRecord",
            Self::UlPartMissing { .. } => "ulPartMissing",
            Self::UlPartSize { .. } => "ulPartSize",
            Self::UlTooManyPartsForSmb(_) => "ulTooManyPartsForSmb",
            Self::CfgLibconfig => "cfgLibconfig",
            Self::CfgUnreadable(_) => "cfgUnreadable",
            Self::CompatibilityBits { .. } => "compatibilityBits",
            Self::VmcMissing { .. } => "vmcMissing",
            Self::VmcInvalid { .. } => "vmcInvalid",
            Self::ArtNotLoaded(_) => "artNotLoaded",
            Self::ArtTooLarge { .. } => "artTooLarge",
            Self::Cheats(_) => "cheats",
            Self::DuplicateStartup(_) => "duplicateStartup",
            Self::UlCfgTrailingBytes(_) => "ulCfgTrailingBytes",
            Self::AppBootMissing(_) => "appBootMissing",
            Self::UnfinishedOperation(_) => "unfinishedOperation",
            Self::Fragmented { .. } => "gameFragmented",
            Self::FragmentedForOpl110 { .. } => "gameFragmentedOpl110",
            Self::VmcFragmented { .. } => "vmcSlotFragmented",
        }
    }
}

impl Warning {
    /// What front ends show (ADR-0029); `Display` is its English.
    pub fn phrase(&self) -> Phrase {
        let phrase = Phrase::notice(self.code());
        match self {
            Self::StaleCache { cached, image } => {
                phrase.with("cached", cached).with("image", image)
            }
            Self::NameIdMismatch { name, image } => phrase.with("name", name).with("image", image),
            Self::UppercaseExtension
            | Self::NonAsciiName
            | Self::ZsoUnsupportedByProfile
            | Self::CfgLibconfig => phrase,
            Self::DuplicateStartup(startup) => phrase.with("startup", startup),
            Self::SmbNameTooLong(len) => phrase.with("length", *len),
            Self::ContainerMismatch { extension, content } => phrase
                .with("extension", extension.to_string())
                .with("content", content.to_string()),
            Self::ZsoHeader(problem) => phrase.with("reason", crate::reasons::zso_problem(problem)),
            Self::CdTooLarge(size) => phrase.with("size", *size),
            Self::UlRecord(anomaly) => phrase.with("reason", crate::reasons::record(anomaly)),
            Self::UlPartMissing { part, name } => phrase.with("part", *part).with("name", name),
            Self::UlPartSize { part, name, size } => phrase
                .with("part", *part)
                .with("name", name)
                .with("size", *size),
            Self::UlTooManyPartsForSmb(parts) => phrase.with("parts", *parts),
            Self::CfgUnreadable(why) => phrase.with("error", why),
            Self::CompatibilityBits { value, allowed } => phrase
                .with("value", value.to_string())
                .with("allowed", format!("{allowed:#x}")),
            // OPL shows the slots as 1 and 2 ($VMC_0 and $VMC_1 in the CFG).
            Self::VmcMissing { slot, name } => phrase.with("slot", *slot + 1).with("name", name),
            Self::VmcInvalid {
                slot,
                name,
                problem,
            } => phrase
                .with("slot", *slot + 1)
                .with("name", name)
                .with("reason", crate::reasons::vmc(problem)),
            Self::ArtNotLoaded(file) => phrase.with("file", file),
            Self::ArtTooLarge {
                file,
                width,
                height,
            } => phrase
                .with("file", file)
                .with("width", *width)
                .with("height", *height),
            Self::Cheats(warning) => phrase.with("reason", crate::reasons::cheat(warning)),
            Self::UlCfgTrailingBytes(extra) => phrase.with("extra", *extra),
            Self::AppBootMissing(boot) => phrase.with("boot", boot),
            Self::UnfinishedOperation(id) => phrase.with("id", id),
            Self::Fragmented { fragments, limit } => {
                phrase.with("fragments", *fragments).with("limit", *limit)
            }
            Self::FragmentedForOpl110 { fragments } => phrase.with("fragments", *fragments),
            Self::VmcFragmented {
                slot,
                name,
                fragments,
            } => phrase
                .with("slot", *slot + 1)
                .with("name", name)
                .with("fragments", *fragments),
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.phrase().english())
    }
}

/// A device root or folder as OPL would list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub root: PathBuf,
    /// Standard OPL folders and whether each exists.
    pub folders: Vec<(String, bool)>,
    pub games: Vec<Game>,
    pub apps: Vec<App>,
    pub skipped: Vec<Skipped>,
    /// Problems not tied to one game.
    pub warnings: Vec<Warning>,
}
