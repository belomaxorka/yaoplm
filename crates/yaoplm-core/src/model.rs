//! What a library looks like to OPL, plus what YAOPLM noticed about it.

use std::fmt;
use std::path::PathBuf;

use yaoplm_formats::Media;
use yaoplm_formats::cfg::Dialect;
use yaoplm_formats::cht::ChtWarning;
use yaoplm_formats::iso_name::NameFormat;
use yaoplm_formats::iso9660::Layers;
use yaoplm_formats::ul_cfg::RecordAnomaly;

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
    pub warnings: Vec<Warning>,
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
    /// OPL mounts it and finds no usable ID; the text says why.
    NoStartup(String),
    Unreadable(String),
    /// `APPS/<folder>` without a usable `title.cfg`.
    BadApp(String),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedExtension(ext) => write!(f, "OPL does not read {ext} files"),
            Self::TitleTooLong => write!(f, "title is longer than 160 characters"),
            Self::NoStartup(why) => write!(f, "OPL hides it: {why}"),
            Self::Unreadable(why) => write!(f, "cannot read it: {why}"),
            Self::BadApp(why) => write!(f, "OPL skips this app: {why}"),
        }
    }
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
    ZsoHeader(String),
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
        problem: String,
    },
    ArtNotLoaded(String),
    ArtTooLarge {
        file: String,
        width: u32,
        height: u32,
    },
    Cheats(ChtWarning),
    /// Several entries share one ID, and with it CFG, ART, CHT and VMC.
    DuplicateStartup,
    UlCfgTrailingBytes(usize),
    AppBootMissing(String),
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
            Self::DuplicateStartup => "duplicateStartup",
            Self::UlCfgTrailingBytes(_) => "ulCfgTrailingBytes",
            Self::AppBootMissing(_) => "appBootMissing",
        }
    }
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleCache { cached, image } => write!(
                f,
                "games.bin still lists this file as {cached}, but the image is {image}; OPL shows {cached} until games.bin is deleted"
            ),
            Self::NameIdMismatch { name, image } => {
                write!(
                    f,
                    "file name says {name} but SYSTEM.CNF says {image}; OPL uses {name}"
                )
            }
            Self::UppercaseExtension => {
                write!(f, "uppercase extension: OPL shows no ISO/ZSO label")
            }
            Self::NonAsciiName => write!(
                f,
                "non-ASCII characters in the name may not display or open on the console"
            ),
            Self::SmbNameTooLong(len) => write!(
                f,
                "file name is {len} bytes; over SMB OPL opens at most 159"
            ),
            Self::ContainerMismatch { extension, content } => {
                write!(f, "extension says {extension} but the file is {content}")
            }
            Self::ZsoUnsupportedByProfile => write!(f, "OPL 1.1.0 cannot read ZSO"),
            Self::ZsoHeader(problem) => write!(f, "ZSO header: {problem}"),
            Self::CdTooLarge(size) => write!(
                f,
                "{size} bytes is more than a PS2 CD holds; it probably belongs in DVD/"
            ),
            Self::UlRecord(anomaly) => write!(f, "ul.cfg record: {anomaly:?}"),
            Self::UlPartMissing { part, name } => write!(f, "chunk {part} ({name}) is missing"),
            Self::UlPartSize { part, size } => {
                write!(f, "chunk {part} is {size} bytes; it must be exactly 1 GiB")
            }
            Self::UlTooManyPartsForSmb(parts) => {
                write!(f, "{parts} chunks: OPL opens at most 10 over SMB")
            }
            Self::CfgLibconfig => write!(
                f,
                "CFG is in wOPL's libconfig format; YAOPLM leaves it alone"
            ),
            Self::CfgUnreadable(why) => write!(f, "cannot read CFG: {why}"),
            Self::CompatibilityBits { value, allowed } => {
                write!(
                    f,
                    "$Compatibility={value} sets bits outside {allowed:#x} for this loader"
                )
            }
            Self::VmcMissing { slot, name } => {
                write!(f, "VMC slot {slot}: VMC/{name}.bin does not exist")
            }
            Self::VmcInvalid {
                slot,
                name,
                problem,
            } => write!(f, "VMC slot {slot}: VMC/{name}.bin is unusable: {problem}"),
            Self::ArtNotLoaded(file) => write!(f, "{file}: this loader only reads PNG art"),
            Self::ArtTooLarge {
                file,
                width,
                height,
            } => {
                write!(
                    f,
                    "{file} is {width}x{height}; OPL rejects art over 1024 px"
                )
            }
            Self::Cheats(w) => write!(f, "cheats: {w:?}"),
            Self::DuplicateStartup => write!(f, "another entry has the same game ID"),
            Self::UlCfgTrailingBytes(n) => {
                write!(
                    f,
                    "ul.cfg ends with {n} stray bytes; OPL shows them as a broken game"
                )
            }
            Self::AppBootMissing(boot) => write!(f, "boot file {boot} is not in the app folder"),
        }
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
