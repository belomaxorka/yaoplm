//! Wire types for volumes and the storage doctor (DEV-1, DEV-2).

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_device::advice::{self, Advice, DiagnoseOptions};
use yaoplm_device::{Bus, FsKind, PartitionStyle, Place, Volume, VolumeKind};

use crate::library::LoaderProfile;
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
    /// How OPL reads the device and why (ADR-0044); the verdict is for this
    /// mode too.
    pub mode: ActiveMode,
    /// Ask which OPL the device is for (ADR-0041, ADR-0044): no signs, a
    /// usable device, and either OPL's folders or a folder read over SMB,
    /// where the console leaves no other sign. A whole system disk without
    /// OPL's folders is not asked: its next step is a folder of its own.
    pub ask_profile: bool,
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
    ) -> Self {
        let key = ActiveProfile::key_for(volume, &display_path(root));
        let mode = ActiveMode::resolve(volume, &key, choice.mode);
        let smb = mode.mode == DeviceMode::Smb;
        let loader = ActiveProfile::resolve(volume, root, choice.profile, smb);
        let options = DiagnoseOptions {
            profile: loader.profile.into(),
            boot_from_device,
            mode: mode.mode.into(),
            whole_volume: volume.is_root(root),
        };
        let found = advice::diagnose(volume, &options);
        let usable = found.iter().all(|a| a.level() != advice::Level::Error);
        let whole_system_disk = found.iter().any(|a| matches!(a, Advice::SystemRoot(_)));
        let ask_profile = usable
            && loader.source == ProfileSource::Detected
            && loader.detected.as_ref().is_some_and(|d| {
                d.confidence == ProfileConfidence::Default
                    && (d.library || (smb && !whole_system_disk))
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

    /// Reads the device at `root` and applies the front end's choice: an
    /// override for the device, then an explicit profile, then the
    /// detection. In a folder read over SMB (`smb`) the loader's files are
    /// copies, and the reason says so (research/18 §3).
    pub fn resolve(
        volume: &Volume,
        root: &std::path::Path,
        choice: ProfileChoice<'_>,
        smb: bool,
    ) -> Self {
        let key = Self::key_for(volume, &display_path(root));
        let mode = if smb {
            yaoplm_device::DeviceMode::Smb
        } else {
            yaoplm_device::DeviceMode::Ps2
        };
        let detected = yaoplm_device::HostFs::new(root)
            .ok()
            .and_then(|fs| yaoplm_core::detect::detect_profile(&fs.with_mode(mode)).ok())
            .map(|mut d| {
                if smb && d.by_loader_files {
                    d.reason =
                        yaoplm_core::Phrase::reason("profileByCopy").with("reason", d.reason);
                }
                DetectedProfile::from(&d)
            });
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

/// How OPL reads a device (ADR-0044).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DeviceMode {
    /// Plugged into the console (USB, MX4SIO, i.LINK).
    #[default]
    Ps2,
    /// Read over the network from a shared folder.
    Smb,
}

impl From<DeviceMode> for yaoplm_device::DeviceMode {
    fn from(mode: DeviceMode) -> Self {
        match mode {
            DeviceMode::Ps2 => Self::Ps2,
            DeviceMode::Smb => Self::Smb,
        }
    }
}

impl From<yaoplm_device::DeviceMode> for DeviceMode {
    fn from(mode: yaoplm_device::DeviceMode) -> Self {
        match mode {
            yaoplm_device::DeviceMode::Ps2 => Self::Ps2,
            yaoplm_device::DeviceMode::Smb => Self::Smb,
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
    /// By the device's place.
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

/// The mode in force for a device and why (ADR-0044).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ActiveMode {
    pub mode: DeviceMode,
    pub source: ModeSource,
    /// The mode the device's place gives.
    pub detected: DeviceMode,
    /// The device's place, to follow a dash: a removable device, a folder
    /// on the computer's disk, a network folder, a folder on the server.
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
    /// The device's place, then the front end's choice: an override for the
    /// device (`key`), then an explicit mode.
    pub fn resolve(volume: &Volume, key: &str, choice: ModeChoice<'_>) -> Self {
        let place = Place::of(volume, choice.server);
        let detected = DeviceMode::from(place.mode());
        let (mode, source) = if let Some(chosen) = choice.overrides.iter().find(|o| o.key == key) {
            (chosen.mode, ModeSource::Override)
        } else if let Some(explicit) = choice.explicit {
            (explicit, ModeSource::Explicit)
        } else {
            (detected, ModeSource::Detected)
        };
        let phrase = place.phrase();
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
