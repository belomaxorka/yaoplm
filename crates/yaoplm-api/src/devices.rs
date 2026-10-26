//! Wire types for volumes and the storage doctor (DEV-1, DEV-2).

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_device::advice::{self, Advice, DiagnoseOptions};
use yaoplm_device::fs::DeviceFs;
use yaoplm_device::{Bus, EmulatorSd, FsKind, PartitionStyle, Place, Volume, VolumeKind};

use yaoplm_core::loaders::installed;

use crate::library::LoaderProfile;
use crate::loaders::{LoaderContext, LoaderTarget};
use crate::paths::display_path;
use crate::phrase::Phrase;
use crate::plans::{NoticeLevel, PlanNotice};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum FileSystem {
    FAT32,
    FAT16,
    #[serde(rename = "exFAT")]
    ExFat,
    NTFS,
    #[serde(rename = "other")]
    Other,
}

impl From<FsKind> for FileSystem {
    fn from(kind: FsKind) -> Self {
        match kind {
            FsKind::Fat32 => Self::FAT32,
            FsKind::Fat16 => Self::FAT16,
            FsKind::ExFat => Self::ExFat,
            FsKind::Ntfs => Self::NTFS,
            FsKind::Other => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DriveKind {
    Removable,
    Fixed,
    Remote,
    Optical,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DriveBus {
    Usb,
    Sd,
    Ieee1394,
    Sata,
    Nvme,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PartitionTable {
    Mbr,
    Gpt,
    /// A file system on the whole device.
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Partition {
    pub table: Option<PartitionTable>,
    pub number: u32,
    pub offset: u64,
    pub mbr_type: Option<u8>,
}

/// A mounted volume and what OPL would make of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub root: String,
    pub label: Option<String>,
    pub file_system: FileSystem,
    /// As the OS reports it.
    pub fs_name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub cluster_size: Option<u32>,
    pub sector_size: Option<u32>,
    pub serial: Option<String>,
    pub kind: DriveKind,
    pub bus: Option<DriveBus>,
    pub model: Option<String>,
    /// USB or SD: what users plug into a PS2.
    pub external: bool,
    pub system: bool,
    pub read_only: bool,
    pub partition: Option<Partition>,
    /// The storage doctor's verdict, worst first (DEV-2).
    pub advice: Vec<PlanNotice>,
    /// macOS `._*` files where OPL trips over them; `InspectDevice` only.
    pub apple_double: Option<u32>,
    /// Journals of unfinished operations; `InspectDevice` only.
    pub unfinished: Vec<String>,
    /// The loader profile in force and why (LIB-1); the doctor's verdict
    /// above is for this profile.
    pub loader: ActiveProfile,
    /// How OPL reads the device and why (ADR-0047); the verdict is for this
    /// mode too.
    pub mode: ActiveMode,
    /// Ask which OPL the device is for (ADR-0041, ADR-0047): no signs, a
    /// usable device, and either OPL's folders or a folder read over SMB,
    /// where the console leaves no other sign. A whole system disk without
    /// OPL's folders is not asked: its next step is a folder of its own.
    pub ask_profile: bool,
    /// The loaders on the device as the catalogue knows them, with what
    /// YAOPLM knows of newer builds, without the network (LDR-4).
    pub installed: Vec<InstalledLoader>,
    /// The device is (on) a memory card emulator's SD (ADR-0048): no loader
    /// goes on it, no format, and its service files are never changed.
    pub emulator: Option<EmulatorInfo>,
}

/// Which firmware family an emulator's files belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum EmulatorKind {
    /// sd2psx and sd2psXtd, on any sd2psx hardware (PSXMemCard Gen2...).
    Sd2psx,
    MemcardPro2,
}

/// One boot card image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BootCard {
    pub channel: u32,
    /// From `BootCard.ini`: says nothing of what the image holds.
    pub name: Option<String>,
    /// Relative to the SD's root.
    pub file: String,
    pub bytes: u64,
    /// What the image holds, read from its own file system; `None` when the
    /// image could not be read.
    pub contents: Option<BootContents>,
}

/// A launcher a boot card starts (names of programs, not translated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub enum BootLauncher {
    FreeMcBoot,
    #[serde(rename = "PS2BBL")]
    Ps2bbl,
    FunTuna,
    #[serde(rename = "FreePSXBoot")]
    FreePsxBoot,
}

/// An OPL-family build inside a boot card, known by its ELF (ADR-0041).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CardLoader {
    /// On the card: `APPS/OPNPS2LD.ELF`.
    pub path: String,
    /// `OPL`, `RiptOPL`, `wOPL`.
    pub family: String,
    pub version: Option<String>,
    /// It reads games from a memory card emulator's SD.
    pub mmce: bool,
}

/// What a boot card holds (research/19 §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct BootContents {
    pub launchers: Vec<BootLauncher>,
    pub loaders: Vec<CardLoader>,
    /// The SD Builder image it is byte for byte: `FMCB 1.966`.
    pub builder: Option<String>,
    /// Its file system could not be read: why (technical).
    pub unreadable: Option<String>,
}

impl From<&yaoplm_core::boot_card::BootContents> for BootContents {
    fn from(c: &yaoplm_core::boot_card::BootContents) -> Self {
        use yaoplm_core::boot_card::Launcher;
        Self {
            launchers: c
                .launchers
                .iter()
                .map(|l| match l {
                    Launcher::FreeMcBoot => BootLauncher::FreeMcBoot,
                    Launcher::Ps2bbl => BootLauncher::Ps2bbl,
                    Launcher::FunTuna => BootLauncher::FunTuna,
                    Launcher::FreePsxBoot => BootLauncher::FreePsxBoot,
                })
                .collect(),
            loaders: c
                .loaders
                .iter()
                .map(|l| CardLoader {
                    path: l.path.clone(),
                    family: l.family.name().to_owned(),
                    version: l.version.clone(),
                    mmce: l.mmce,
                })
                .collect(),
            builder: c
                .builder
                .as_ref()
                .and_then(|b| b.names.first())
                .map(|n| (*n).to_owned()),
            unreadable: c.unreadable.clone(),
        }
    }
}

/// What each boot card of a survey holds, by variant and channel.
type Held = Vec<Vec<Option<yaoplm_core::boot_card::BootContents>>>;

/// Boot card contents read so far, by image path, size and modification
/// time: the devices screen asks again on every refresh, and reading a boot
/// card means hashing its whole image.
fn cached_contents(
    fs: &yaoplm_device::HostFs,
    file: &str,
    len: u64,
) -> Option<yaoplm_core::boot_card::BootContents> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock, PoisonError};
    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);
    static CACHE: OnceLock<Mutex<HashMap<Key, yaoplm_core::boot_card::BootContents>>> =
        OnceLock::new();
    let rel = yaoplm_device::RelPath::new(file).ok()?;
    let path = fs.resolve(&rel);
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let key = (path, len, modified);
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&key)
    {
        return Some(found.clone());
    }
    let found = yaoplm_core::boot_card::contents(fs, &rel, len).ok()?;
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(key, found.clone());
    Some(found)
}

/// The cards of one variant folder: `PS2`, `PS1`, `COH`, `SC2`, `PROT`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CardVariant {
    pub name: String,
    pub boot: Vec<BootCard>,
    pub max_channels: Option<u32>,
    pub card_size: Option<u32>,
    /// `Card<N>` folders.
    pub cards: u32,
    /// Folders the firmware made for a game ID.
    pub game_cards: u32,
}

/// A `[section]` of an ini file, its entries as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IniSection {
    pub name: String,
    pub entries: Vec<IniEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct IniEntry {
    pub key: String,
    pub value: String,
}

/// A memory card emulator's SD as the device card shows it (ADR-0048).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct EmulatorInfo {
    pub kinds: Vec<EmulatorKind>,
    /// The files that told, relative to the SD's root.
    pub signs: Vec<String>,
    /// Where the opened folder lies on the SD; empty at its root.
    pub folder: String,
    /// Top-level names YAOPLM never changes.
    pub protected: Vec<String>,
    pub variants: Vec<CardVariant>,
    /// `.sd2psx/settings.ini` as the firmware reads it.
    pub settings: Vec<IniSection>,
    /// Images the firmware moved aside as damaged.
    pub damaged: Vec<String>,
}

impl EmulatorInfo {
    fn new(sd: &EmulatorSd, survey: Option<&yaoplm_core::emulator::Survey>, held: &Held) -> Self {
        let mut kinds = Vec::new();
        if sd.sd2psx {
            kinds.push(EmulatorKind::Sd2psx);
        }
        if sd.mcp2 {
            kinds.push(EmulatorKind::MemcardPro2);
        }
        let variant = |(i, v): (usize, &yaoplm_core::emulator::Variant)| CardVariant {
            name: v.name.clone(),
            boot: v
                .boot
                .iter()
                .enumerate()
                .map(|(j, b)| BootCard {
                    channel: b.channel,
                    name: b.name.clone(),
                    file: b.file.clone(),
                    bytes: b.bytes,
                    contents: held
                        .get(i)
                        .and_then(|v| v.get(j))
                        .and_then(Option::as_ref)
                        .map(BootContents::from),
                })
                .collect(),
            max_channels: v.max_channels,
            card_size: v.card_size,
            cards: v.cards,
            game_cards: v.game_cards,
        };
        let section = |(name, entries): (String, Vec<(String, String)>)| IniSection {
            name,
            entries: entries
                .into_iter()
                .map(|(key, value)| IniEntry { key, value })
                .collect(),
        };
        Self {
            kinds,
            signs: sd.signs.clone(),
            folder: sd.at.clone(),
            protected: sd.service_files().iter().map(|n| (*n).to_owned()).collect(),
            variants: survey
                .map(|s| s.variants.iter().enumerate().map(variant).collect())
                .unwrap_or_default(),
            settings: survey
                .and_then(|s| s.settings.as_ref())
                .map(|ini| ini.sections().into_iter().map(section).collect())
                .unwrap_or_default(),
            damaged: survey.map(|s| s.damaged.clone()).unwrap_or_default(),
        }
    }
}

/// The first OPL build with MMCE inside a boot card, PS2 channels first,
/// becomes the detection's answer: its family's rules (RiptOPL's own, else
/// 1.2.0 beta's) by an indirect sign.
fn by_boot_card(
    detection: &mut yaoplm_core::detect::Detection,
    survey: Option<&yaoplm_core::emulator::Survey>,
    held: &Held,
) {
    let Some(survey) = survey else {
        return;
    };
    for (variant, held) in survey.variants.iter().zip(held) {
        for (boot, contents) in variant.boot.iter().zip(held) {
            let Some(loader) = contents
                .as_ref()
                .and_then(|c| c.loaders.iter().find(|l| l.mmce))
            else {
                continue;
            };
            detection.profile = match loader.family {
                yaoplm_core::detect::Family::RiptOpl => yaoplm_core::LoaderProfile::RiptOpl,
                _ => yaoplm_core::LoaderProfile::Opl120Beta,
            };
            detection.confidence = yaoplm_core::detect::Confidence::Indirect;
            detection.reason = yaoplm_core::Phrase::reason("profileByBootCard")
                .with(
                    "version",
                    loader.version.as_deref().unwrap_or(loader.family.name()),
                )
                .with("file", &loader.path)
                .with("channel", boot.channel)
                .with("variant", &variant.name);
            return;
        }
    }
}

/// The SD's root for a device at `root` that lies `at` deep on it.
fn sd_root(root: &std::path::Path, at: &str) -> std::path::PathBuf {
    let depth = at.split('/').filter(|c| !c.is_empty()).count();
    root.ancestors().nth(depth).unwrap_or(root).to_path_buf()
}

/// How YAOPLM knows which build a loader is (LDR-4, ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderKnownBy {
    /// YAOPLM installed it, and the file is still the one it installed.
    Manifest,
    /// The SHA-256 of a frozen upstream release.
    Checksum,
    /// The version string inside the ELF.
    Version,
    /// Only its name: the family, not the build.
    Name,
}

/// How an installed build stands against the newest of its channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum LoaderFreshness {
    /// Nothing newer is known.
    Current,
    /// The channel has a newer build: `latest`.
    Update,
    /// No release was read yet, the build is not in the catalogue, or the
    /// versions do not order.
    Unknown,
}

/// What "Roll back" would put back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct LoaderBackup {
    pub file: String,
    /// `OPL`; `null`: not a loader YAOPLM knows.
    pub family: Option<String>,
    pub version: Option<String>,
}

/// A loader on the device (LDR-4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct InstalledLoader {
    pub path: String,
    /// `OPL`, `RiptOPL`.
    pub family: String,
    pub version: Option<String>,
    pub sha256: String,
    /// The catalogue build, when known.
    pub build: Option<String>,
    pub known_by: LoaderKnownBy,
    /// The manifest names another file here: it was replaced without
    /// YAOPLM.
    pub manifest_stale: bool,
    /// From the manifest, while it holds.
    pub preset: Option<String>,
    pub device: Option<LoaderTarget>,
    pub language: Option<String>,
    /// The newest version of the build's channel YAOPLM read.
    pub latest: Option<String>,
    /// When that was read, milliseconds since 1970.
    pub checked_at: Option<u64>,
    pub freshness: LoaderFreshness,
    pub backup: Option<LoaderBackup>,
}

impl InstalledLoader {
    fn new(loader: &installed::Installed, context: &LoaderContext) -> Self {
        let latest = loader
            .build
            .as_ref()
            .and_then(|build| context.latest.get(build));
        let freshness = match installed::freshness(
            loader.version.as_deref(),
            latest.map(|l| l.version.as_str()),
        ) {
            installed::Freshness::Current => LoaderFreshness::Current,
            installed::Freshness::Update(_) => LoaderFreshness::Update,
            installed::Freshness::Unknown => LoaderFreshness::Unknown,
        };
        let entry = loader.entry.as_ref();
        Self {
            path: loader.path.clone(),
            family: loader.family.name().to_owned(),
            version: loader.version.clone(),
            sha256: loader.sha256.clone(),
            build: loader.build.clone(),
            known_by: match loader.known_by {
                installed::KnownBy::Manifest => LoaderKnownBy::Manifest,
                installed::KnownBy::Checksum => LoaderKnownBy::Checksum,
                installed::KnownBy::Version => LoaderKnownBy::Version,
                installed::KnownBy::Name => LoaderKnownBy::Name,
            },
            manifest_stale: loader.manifest_stale,
            preset: entry.and_then(|e| e.preset.clone()),
            device: entry.map(|e| match e.device.as_str() {
                "mx4sio" => LoaderTarget::Mx4sio,
                _ => LoaderTarget::Usb,
            }),
            language: entry.and_then(|e| e.language.clone()),
            latest: latest.map(|l| l.version.clone()),
            checked_at: latest
                .and_then(|l| l.checked_at)
                .map(|s| s.saturating_mul(1000)),
            freshness,
            backup: loader.backup.as_ref().map(|b| LoaderBackup {
                file: b.file.clone(),
                family: b.family.map(|f| f.name().to_owned()),
                version: b.version.clone(),
            }),
        }
    }
}

fn notice(advice: &Advice) -> PlanNotice {
    PlanNotice {
        level: match advice.level() {
            advice::Level::Info => NoticeLevel::Info,
            advice::Level::Warning => NoticeLevel::Warning,
            advice::Level::Error => NoticeLevel::Error,
        },
        code: advice.code().to_owned(),
        message: advice.to_string(),
        phrase: Some(Phrase::from(&advice.phrase())),
    }
}

impl DeviceInfo {
    /// The device at `root` (a volume root, or a folder on `volume`), with
    /// the doctor's verdict for the profile and the mode in force there.
    pub fn new(
        volume: &Volume,
        root: &std::path::Path,
        choice: DeviceChoice<'_>,
        boot_from_device: bool,
        loaders: Option<&LoaderContext>,
    ) -> Self {
        let key = ActiveProfile::key_for(volume, &display_path(root));
        let host = yaoplm_device::HostFs::new(root).ok();
        let sd = host.as_ref().and_then(|fs| fs.emulator().ok().flatten());
        let mode = ActiveMode::resolve(volume, &key, choice.mode, sd.as_ref());
        let smb = mode.mode == DeviceMode::Smb;
        let fs = host.map(|fs| fs.with_mode(mode.mode.into()));
        // What the SD holds, read from its root even when a folder on it is
        // open.
        let sd_fs = sd
            .as_ref()
            .and_then(|sd| yaoplm_device::HostFs::new(&sd_root(fs.as_ref()?.root(), &sd.at)).ok());
        let survey = sd_fs
            .as_ref()
            .and_then(|sd_fs| yaoplm_core::emulator::survey(sd_fs).ok());
        // What each boot card holds, from its own file system.
        let held: Held = match (&sd_fs, &survey) {
            (Some(sd_fs), Some(survey)) => survey
                .variants
                .iter()
                .map(|v| {
                    v.boot
                        .iter()
                        .map(|b| cached_contents(sd_fs, &b.file, b.bytes))
                        .collect()
                })
                .collect(),
            _ => Vec::new(),
        };
        let emulator_mode = mode.mode == DeviceMode::Emulator;
        // In a folder read over SMB the loader's files are copies, and the
        // reason says so (research/18 §3). On a memory card emulator's SD
        // with no other sign, an OPL build with MMCE inside a boot card
        // answers which OPL reads it (ADR-0048).
        let detection = fs
            .as_ref()
            .and_then(|fs| yaoplm_core::detect::detect_profile(fs).ok())
            .map(|mut d| {
                if smb && d.by_loader_files {
                    d.reason =
                        yaoplm_core::Phrase::reason("profileByCopy").with("reason", d.reason);
                }
                if emulator_mode && d.confidence == yaoplm_core::detect::Confidence::Default {
                    by_boot_card(&mut d, survey.as_ref(), &held);
                }
                d
            });
        let loader = ActiveProfile::resolve(volume, root, choice.profile, detection.as_ref());
        let installed = match (&fs, &detection, loaders) {
            (Some(fs), Some(detection), Some(context)) => {
                installed::installed(fs, detection, &context.catalog)
                    .map(|found| {
                        found
                            .iter()
                            .map(|l| InstalledLoader::new(l, context))
                            .collect()
                    })
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        };
        let options = DiagnoseOptions {
            profile: loader.profile.into(),
            boot_from_device,
            mode: mode.mode.into(),
            whole_volume: volume.is_root(root),
        };
        let mut found = advice::diagnose(volume, &options);
        if let Some(survey) = &survey {
            found.extend(survey.advice.iter().cloned());
            for (variant, held) in survey.variants.iter().zip(&held) {
                for (boot, contents) in variant.boot.iter().zip(held) {
                    found.extend(contents.as_ref().and_then(|c| {
                        yaoplm_core::boot_card::channel_advice(
                            &variant.name,
                            boot.channel,
                            boot.name.as_deref(),
                            c,
                        )
                    }));
                }
            }
            found.sort_by_key(|a| std::cmp::Reverse(a.level()));
        }
        let usable = found.iter().all(|a| a.level() != advice::Level::Error);
        let whole_system_disk = found.iter().any(|a| matches!(a, Advice::SystemRoot(_)));
        // A memory card emulator's SD is asked even without OPL's folders:
        // the answer decides whether they are offered (ADR-0048).
        let ask_profile = usable
            && loader.source == ProfileSource::Detected
            && loader.detected.as_ref().is_some_and(|d| {
                d.confidence == ProfileConfidence::Default
                    && (d.library || emulator_mode || (smb && !whole_system_disk))
            });
        Self {
            root: display_path(root),
            label: volume.label.clone(),
            file_system: volume.fs.into(),
            fs_name: volume.fs_name.clone(),
            total_bytes: volume.total_bytes,
            free_bytes: volume.free_bytes,
            cluster_size: volume.cluster_size,
            sector_size: volume.sector_size,
            serial: volume.serial.clone(),
            kind: match volume.kind {
                VolumeKind::Removable => DriveKind::Removable,
                VolumeKind::Fixed => DriveKind::Fixed,
                VolumeKind::Remote => DriveKind::Remote,
                VolumeKind::Optical => DriveKind::Optical,
                VolumeKind::Unknown => DriveKind::Unknown,
            },
            bus: volume.bus.map(|bus| match bus {
                Bus::Usb => DriveBus::Usb,
                Bus::Sd => DriveBus::Sd,
                Bus::Ieee1394 => DriveBus::Ieee1394,
                Bus::Sata => DriveBus::Sata,
                Bus::Nvme => DriveBus::Nvme,
                Bus::Other => DriveBus::Other,
            }),
            model: volume.model.clone(),
            external: volume.is_external(),
            system: volume.system,
            read_only: volume.read_only,
            partition: volume.partition.map(|p| Partition {
                table: p.style.map(|style| match style {
                    PartitionStyle::Mbr => PartitionTable::Mbr,
                    PartitionStyle::Gpt => PartitionTable::Gpt,
                    PartitionStyle::None => PartitionTable::None,
                }),
                number: p.number,
                offset: p.offset,
                mbr_type: p.mbr_type,
            }),
            advice: found.iter().map(notice).collect(),
            apple_double: None,
            unfinished: Vec::new(),
            loader,
            mode,
            ask_profile,
            installed,
            emulator: sd
                .as_ref()
                .map(|sd| EmulatorInfo::new(sd, survey.as_ref(), &held)),
        }
    }

    /// Adds findings that come from the folder rather than the volume.
    pub fn add_apple_double(&mut self, count: usize) {
        if count > 0 {
            self.advice.push(notice(&Advice::AppleDouble(count)));
        }
        self.apple_double = Some(u32::try_from(count).unwrap_or(u32::MAX));
    }
}

/// How sure the detection is (SPEC §5.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ProfileConfidence {
    /// A release known by its checksum or by the version inside its ELF.
    Exact,
    /// File names, settings files, the file system or the images.
    Indirect,
    /// No signs: the default profile.
    Default,
}

/// Where the profile in force comes from (ADR-0023, ADR-0040).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ProfileSource {
    /// Chosen for this device.
    Override,
    /// Forced for every device by the request (the CLI's `--profile`).
    Explicit,
    /// Read from the device.
    Detected,
}

/// A loader found on the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct FoundLoader {
    /// `OPL`, `RiptOPL`, `wOPL`, `OPL MMCE`, `NHDDL`.
    pub family: String,
    pub version: Option<String>,
    pub path: String,
    /// Known by its SHA-256.
    pub exact: bool,
}

/// What the device's files say (LIB-1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DetectedProfile {
    pub profile: LoaderProfile,
    pub confidence: ProfileConfidence,
    /// To follow a colon after the profile's name.
    pub reason: Phrase,
    /// `reason` in English.
    pub reason_text: String,
    pub loaders: Vec<FoundLoader>,
    pub notices: Vec<PlanNotice>,
    /// OPL's folders or `ul.cfg` are at the root (the device is prepared for
    /// OPL): with no signs, the interface asks which OPL it is for.
    pub library: bool,
}

impl From<&yaoplm_core::detect::Detection> for DetectedProfile {
    fn from(d: &yaoplm_core::detect::Detection) -> Self {
        use yaoplm_core::detect::Confidence;
        Self {
            profile: d.profile.into(),
            confidence: match d.confidence {
                Confidence::Exact => ProfileConfidence::Exact,
                Confidence::Indirect => ProfileConfidence::Indirect,
                Confidence::Default => ProfileConfidence::Default,
            },
            reason: Phrase::from(&d.reason),
            reason_text: d.reason.english(),
            loaders: d
                .loaders
                .iter()
                .map(|l| FoundLoader {
                    family: l.family.name().to_owned(),
                    version: l.version.clone(),
                    path: l.path.clone(),
                    exact: l.exact,
                })
                .collect(),
            notices: d.notices.iter().map(PlanNotice::from).collect(),
            library: d.library,
        }
    }
}

/// A profile chosen for one device: `key` is [`ActiveProfile::key`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ProfileOverride {
    pub key: String,
    pub profile: LoaderProfile,
}

/// The loader profile in force for a device and why (LIB-1, ADR-0023).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ActiveProfile {
    pub profile: LoaderProfile,
    pub source: ProfileSource,
    /// What overrides are keyed by: `serial:<volume serial>` for a USB or
    /// SD volume, so a stick keeps its profile under another drive letter;
    /// `path:<root>` for folders and shares.
    pub key: String,
    /// `None` when the device could not be read.
    pub detected: Option<DetectedProfile>,
}

/// How a front end chooses the profile: `overrides` are those chosen per
/// device in the interface, `explicit` one forced for every device by the
/// CLI's `--profile` (`None`: auto). The interface has no global profile
/// (ADR-0040).
#[derive(Debug, Clone, Copy)]
pub struct ProfileChoice<'a> {
    pub explicit: Option<LoaderProfile>,
    pub overrides: &'a [ProfileOverride],
}

impl ActiveProfile {
    /// The key of a device shown at `root`.
    pub fn key_for(volume: &Volume, root: &str) -> String {
        match &volume.serial {
            Some(serial) if volume.is_external() => format!("serial:{serial}"),
            _ => format!("path:{root}"),
        }
    }

    /// Applies the front end's choice to what the device at `root` says:
    /// an override for the device, then an explicit profile, then the
    /// detection (`None`: the device could not be read).
    pub fn resolve(
        volume: &Volume,
        root: &std::path::Path,
        choice: ProfileChoice<'_>,
        detection: Option<&yaoplm_core::detect::Detection>,
    ) -> Self {
        let key = Self::key_for(volume, &display_path(root));
        let detected = detection.map(DetectedProfile::from);
        let (profile, source) = if let Some(chosen) = choice.overrides.iter().find(|o| o.key == key)
        {
            (chosen.profile, ProfileSource::Override)
        } else if let Some(explicit) = choice.explicit {
            (explicit, ProfileSource::Explicit)
        } else {
            let detected_profile = detected.as_ref().map(|d| d.profile);
            (
                detected_profile.unwrap_or_default(),
                ProfileSource::Detected,
            )
        };
        Self {
            profile,
            source,
            key,
            detected,
        }
    }
}

/// How OPL reads a device (ADR-0047).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DeviceMode {
    /// Plugged into the console (USB, MX4SIO, i.LINK).
    #[default]
    Ps2,
    /// Read over the network from a shared folder.
    Smb,
    /// The SD of a memory card emulator, read by OPL builds with MMCE
    /// (ADR-0048).
    Emulator,
}

impl From<DeviceMode> for yaoplm_device::DeviceMode {
    fn from(mode: DeviceMode) -> Self {
        match mode {
            DeviceMode::Ps2 => Self::Ps2,
            DeviceMode::Smb => Self::Smb,
            DeviceMode::Emulator => Self::Emulator,
        }
    }
}

impl From<yaoplm_device::DeviceMode> for DeviceMode {
    fn from(mode: yaoplm_device::DeviceMode) -> Self {
        match mode {
            yaoplm_device::DeviceMode::Ps2 => Self::Ps2,
            yaoplm_device::DeviceMode::Smb => Self::Smb,
            yaoplm_device::DeviceMode::Emulator => Self::Emulator,
        }
    }
}

/// Where the mode in force comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ModeSource {
    /// Chosen for this device.
    Override,
    /// Forced by the request (the CLI's `--mode`).
    Explicit,
    /// By the device's place, or a memory card emulator's files.
    Detected,
}

/// A mode chosen for one device, keyed like [`ActiveProfile::key`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ModeOverride {
    pub key: String,
    pub mode: DeviceMode,
}

/// The mode in force for a device and why (ADR-0047).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ActiveMode {
    pub mode: DeviceMode,
    pub source: ModeSource,
    /// The mode the device's place gives, or a memory card emulator's
    /// files.
    pub detected: DeviceMode,
    /// The device's place, to follow a dash: a removable device, a folder
    /// on the computer's disk, a network folder, a folder on the server; or
    /// the files of a memory card emulator.
    pub place: Phrase,
    /// `place` in English.
    pub place_text: String,
}

/// How a front end chooses the mode: `overrides` chosen per device in the
/// interface, `explicit` forced by the CLI's `--mode` (`None`: by place).
/// `server`: the device is a root of `yaoplm-server`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModeChoice<'a> {
    pub explicit: Option<DeviceMode>,
    pub overrides: &'a [ModeOverride],
    pub server: bool,
}

impl ActiveMode {
    /// The device's place (a memory card emulator's files, `sd`, come
    /// first: a card reader cannot tell, ADR-0048), then the front end's
    /// choice: an override for the device (`key`), then an explicit mode.
    pub fn resolve(
        volume: &Volume,
        key: &str,
        choice: ModeChoice<'_>,
        sd: Option<&EmulatorSd>,
    ) -> Self {
        let place = Place::of(volume, choice.server);
        let detected = match sd {
            Some(_) => DeviceMode::Emulator,
            None => DeviceMode::from(place.mode()),
        };
        let (mode, source) = if let Some(chosen) = choice.overrides.iter().find(|o| o.key == key) {
            (chosen.mode, ModeSource::Override)
        } else if let Some(explicit) = choice.explicit {
            (explicit, ModeSource::Explicit)
        } else {
            (detected, ModeSource::Detected)
        };
        let phrase = match sd {
            Some(sd) => {
                // `.sd2psx, MemoryCards`: the variant folders are details.
                let mut top: Vec<&str> = Vec::new();
                for sign in &sd.signs {
                    let first = sign.split('/').next().unwrap_or(sign);
                    if !top.contains(&first) {
                        top.push(first);
                    }
                }
                yaoplm_core::Phrase::reason("placeEmulator").with("files", top.join(", "))
            }
            None => place.phrase(),
        };
        Self {
            mode,
            source,
            detected,
            place_text: phrase.english(),
            place: Phrase::from(&phrase),
        }
    }
}

/// Everything a front end chooses about the devices it lists or inspects.
#[derive(Debug, Clone, Copy)]
pub struct DeviceChoice<'a> {
    pub profile: ProfileChoice<'a>,
    pub mode: ModeChoice<'a>,
}
