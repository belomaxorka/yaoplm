//! "Storage doctor": a read-only verdict on a volume for a loader (DEV-2).
//!
//! Rules from research/06 §1 and §6.1 (OPL `b3a2199`, ps2sdk `329e2bb`,
//! FatFs `fjtrujy/FatFs@iop-r0.16`):
//! - BDM reads FAT32 and exFAT only; exFAT needs OPL 1.2.0 beta (rev1880).
//! - FatFs is built for 512-byte logical sectors only.
//! - FAT32 holds files up to 4 GiB - 1, so larger games become `ul` chunks.
//! - FMCB 1.966 and legacy uLaunchELF cannot read exFAT or GPT, so a stick
//!   that also boots OPL must be FAT32 on MBR.
//! - OPL 1.1.0 has no GPT support. Superfloppies (no partition table) were
//!   broken in OPL builds between about rev1981 and April 2025.
//! - exFAT clusters of 512 KiB and more are reported to cause problems; on
//!   FAT32 small clusters make every game start walk a longer FAT chain.
//!
//! A folder OPL reads over SMB (ADR-0047, research/18) is not mounted by
//! BDM: none of the above applies there, only SMB's own limits and the 4 GiB
//! file size of a FAT volume on the computer.
//!
//! Nor is a memory card emulator's SD (ADR-0048, research/19 §5): its
//! firmware mounts it, and OPL reads files through MMCE. sd2psXtd 1.4.0
//! mounts MBR partition 1 only, FAT or exFAT (SdFat `SDFAT_FILE_TYPE 3`,
//! `sd.begin()`; sd2psXtd/firmware@8a20bd9); its unreleased `develop` adds
//! GPT and cards without a partition table (PR #112). Clusters and
//! fragments do not matter there.

use std::fmt;
use std::fs;
use std::path::Path;

use yaoplm_formats::LoaderProfile;
use yaoplm_i18n::Phrase;

use crate::mode::DeviceMode;
use crate::volume::{FsKind, PartitionStyle, Volume};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Info,
    Warning,
    /// OPL will not see games on this volume, or YAOPLM must not write it.
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiagnoseOptions {
    pub profile: LoaderProfile,
    /// OPL itself is started from this device through FreeMcBoot 1.966 or
    /// an old uLaunchELF, which read FAT32 only.
    pub boot_from_device: bool,
    /// How OPL reads the device (ADR-0047).
    pub mode: DeviceMode,
    /// The device is the whole volume (a drive or mount point), not a
    /// folder on it.
    pub whole_volume: bool,
}

/// One finding about a volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Advice {
    /// The volume holds the running OS.
    SystemVolume,
    ReadOnly,
    /// A local file system OPL's BDM driver cannot mount.
    FsUnreadable(FsKind),
    /// OPL 1.1.0 has no exFAT driver.
    ExfatNeedsOpl12,
    /// FMCB 1.966 and legacy uLaunchELF cannot start OPL from exFAT.
    FatNeededToBoot,
    /// Games over 4 GiB are stored as `ul` chunks on FAT32.
    LargeGamesAsUl,
    /// exFAT stores games over 4 GiB as one image (OPL 1.2.0 beta).
    ExfatRecommended,
    /// OPL reads the device over SMB: SMBv1 only (RiptOPL also SMB2),
    /// image names up to 159 bytes, at most 10 `ul` parts (research/18).
    Smb {
        smb2: bool,
    },
    /// A whole system disk shared as the library: a separate folder is
    /// cleaner (OPL creates its folders at the share's root).
    SystemRoot(String),
    /// Less than a tenth of the volume, or less than a single-layer DVD,
    /// is free (owner's decision 2026-10-25, ADR-0047).
    LowSpace {
        free: u64,
        total: u64,
    },
    /// FatFs in OPL is built for 512-byte sectors only.
    SectorSize(u32),
    ClusterTooLarge(u32),
    /// FAT32 clusters below 16 KiB: OPL walks a long FAT chain at launch.
    ClusterSmall(u32),
    /// OPL 1.1.0 and legacy loaders read MBR only.
    GptUnsupported,
    /// No partition table: broken in some OPL 1.2.0 builds.
    NoPartitionTable,
    /// The partition does not start on a 1 MiB boundary.
    Misaligned(u64),
    /// Not a USB or SD device. Fine for a folder or share, but OPL reads
    /// BDM games from USB, MX4SIO, iLink or an internal HDD only.
    NotExternal,
    /// macOS `._*` files in `CD/`, `DVD/`, `LNG/` or `APPS/`: OPL probes
    /// them on every scan, and `._lang_*.lng` shows up as a language.
    AppleDouble(usize),
    /// A memory card emulator's firmware mounts MBR only (sd2psXtd 1.4.0).
    EmulatorGpt,
    EmulatorNoPartitionTable,
    /// The data is not in partition 1, the only one the firmware mounts.
    EmulatorPartition(u32),
    /// The firmware reads FAT and exFAT only.
    EmulatorFs(FsKind),
    /// A copy of an emulator's SD on the computer: the firmware reads only
    /// the SD itself, so no rule of the SD applies to the copy.
    EmulatorCopy,
    /// `.sd2psx/settings.ini` makes new cards larger than 8 MiB; the
    /// firmware's README recommends 8 MiB: larger ones may get corrupted on
    /// a slow SD.
    EmulatorCardSize(u32),
    /// Images the firmware could not use and moved aside as
    /// `<folder>-<channel>.crp.<n>` (`ps2_cardman.c`).
    EmulatorDamaged {
        count: usize,
        files: String,
    },
    /// An ini file whose last line has no line feed: the firmware never
    /// reads that line.
    EmulatorIniTail {
        file: String,
        line: String,
    },
    /// A boot channel's name in `BootCard.ini` says something its image
    /// does not hold (research/19 §4).
    EmulatorChannelName {
        variant: String,
        channel: u32,
        name: String,
        holds: String,
    },
}

impl Advice {
    pub fn level(&self) -> Level {
        match self {
            Self::SystemVolume
            | Self::ReadOnly
            | Self::FsUnreadable(_)
            | Self::ExfatNeedsOpl12
            | Self::SectorSize(_)
            | Self::GptUnsupported
            | Self::EmulatorGpt
            | Self::EmulatorNoPartitionTable
            | Self::EmulatorPartition(_)
            | Self::EmulatorFs(_) => Level::Error,
            Self::FatNeededToBoot
            | Self::ClusterTooLarge(_)
            | Self::NoPartitionTable
            | Self::SystemRoot(_)
            | Self::LowSpace { .. }
            | Self::AppleDouble(_)
            | Self::EmulatorDamaged { .. }
            | Self::EmulatorIniTail { .. } => Level::Warning,
            // The owner cannot change these in YAOPLM, and the SD works as it is.
            Self::EmulatorCardSize(_)
            | Self::EmulatorChannelName { .. }
            | Self::LargeGamesAsUl
            | Self::ExfatRecommended
            | Self::Smb { .. }
            | Self::ClusterSmall(_)
            | Self::Misaligned(_)
            | Self::NotExternal
            | Self::EmulatorCopy => Level::Info,
        }
    }

    /// Stable identifier for front ends (i18n keys).
    pub fn code(&self) -> &'static str {
        match self {
            Self::SystemVolume => "systemVolume",
            Self::ReadOnly => "readOnly",
            Self::FsUnreadable(_) => "fsUnreadable",
            Self::ExfatNeedsOpl12 => "exfatNeedsOpl12",
            Self::FatNeededToBoot => "fatNeededToBoot",
            Self::LargeGamesAsUl => "largeGamesAsUl",
            Self::ExfatRecommended => "exfatRecommended",
            Self::Smb { smb2: false } => "smbRules",
            Self::Smb { smb2: true } => "smbRulesSmb2",
            Self::SystemRoot(_) => "systemRoot",
            Self::LowSpace { .. } => "lowSpace",
            Self::SectorSize(_) => "sectorSize",
            Self::ClusterTooLarge(_) => "clusterTooLarge",
            Self::ClusterSmall(_) => "clusterSmall",
            Self::GptUnsupported => "gptUnsupported",
            Self::NoPartitionTable => "noPartitionTable",
            Self::Misaligned(_) => "misaligned",
            Self::NotExternal => "notExternal",
            Self::AppleDouble(_) => "appleDouble",
            Self::EmulatorGpt => "emulatorGpt",
            Self::EmulatorNoPartitionTable => "emulatorNoPartitionTable",
            Self::EmulatorPartition(_) => "emulatorPartition",
            Self::EmulatorFs(_) => "emulatorFs",
            Self::EmulatorCopy => "emulatorCopy",
            Self::EmulatorCardSize(_) => "emulatorCardSize",
            Self::EmulatorDamaged { .. } => "emulatorDamaged",
            Self::EmulatorIniTail { .. } => "emulatorIniTail",
            Self::EmulatorChannelName { .. } => "emulatorChannelName",
        }
    }
}

fn kib(bytes: u32) -> u32 {
    bytes / 1024
}

impl Advice {
    /// What front ends show (ADR-0029); `Display` is its English.
    pub fn phrase(&self) -> Phrase {
        let phrase = Phrase::notice(self.code());
        match self {
            Self::FsUnreadable(fs) | Self::EmulatorFs(fs) => phrase.with("fs", fs.to_string()),
            Self::EmulatorPartition(number) => phrase.with("number", *number),
            Self::EmulatorCardSize(size) => phrase.with("size", *size),
            Self::EmulatorDamaged { count, files } => {
                phrase.with("count", *count).with("files", files)
            }
            Self::EmulatorIniTail { file, line } => phrase.with("file", file).with("line", line),
            Self::EmulatorChannelName {
                variant,
                channel,
                name,
                holds,
            } => phrase
                .with("variant", variant)
                .with("channel", *channel)
                .with("name", name)
                .with("holds", holds),
            Self::SectorSize(size) => phrase.with("size", *size),
            Self::ClusterTooLarge(size) | Self::ClusterSmall(size) => {
                phrase.with("kib", kib(*size))
            }
            Self::Misaligned(offset) => phrase.with("offset", *offset),
            Self::AppleDouble(files) => phrase.with("files", *files),
            Self::SystemRoot(folder) => phrase.with("folder", folder),
            Self::LowSpace { free, total } => phrase.with("free", *free).with("total", *total),
            Self::SystemVolume
            | Self::ReadOnly
            | Self::ExfatNeedsOpl12
            | Self::FatNeededToBoot
            | Self::LargeGamesAsUl
            | Self::ExfatRecommended
            | Self::Smb { .. }
            | Self::GptUnsupported
            | Self::NoPartitionTable
            | Self::NotExternal
            | Self::EmulatorGpt
            | Self::EmulatorNoPartitionTable
            | Self::EmulatorCopy => phrase,
        }
    }
}

impl fmt::Display for Advice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.phrase().english())
    }
}

const MIB: u64 = 1 << 20;

/// The folder a whole system disk's warning suggests sharing instead.
pub const SHARE_FOLDER: &str = "PS2";

/// A single-layer DVD: less free space than this takes no more DVD games.
const DVD5_BYTES: u64 = 4_700_000_000;

/// Little free space, on any device in either mode. A volume nothing can be
/// written to fills no further.
fn low_space(volume: &Volume) -> Option<Advice> {
    let (free, total) = (volume.free_bytes, volume.total_bytes);
    (!volume.read_only && total > 0 && (free < DVD5_BYTES || free.saturating_mul(10) < total))
        .then_some(Advice::LowSpace { free, total })
}

/// Everything worth telling about `volume` for `options`, worst first.
pub fn diagnose(volume: &Volume, options: &DiagnoseOptions) -> Vec<Advice> {
    match options.mode {
        DeviceMode::Ps2 => diagnose_ps2(volume, options),
        DeviceMode::Smb => diagnose_smb(volume, options),
        DeviceMode::Emulator => diagnose_emulator(volume),
    }
}

/// A memory card emulator's SD: what its firmware mounts (ADR-0048). What
/// its files say (card size, damaged images) comes from reading them.
fn diagnose_emulator(volume: &Volume) -> Vec<Advice> {
    let mut found = Vec::new();
    if volume.read_only {
        found.push(Advice::ReadOnly);
    }
    found.extend(low_space(volume));
    if !volume.is_external() {
        found.push(Advice::EmulatorCopy);
        return sorted(found);
    }
    if !matches!(volume.fs, FsKind::Fat32 | FsKind::Fat16 | FsKind::ExFat) {
        found.push(Advice::EmulatorFs(volume.fs));
    }
    if let Some(partition) = volume.partition {
        match partition.style {
            Some(PartitionStyle::Gpt) => found.push(Advice::EmulatorGpt),
            Some(PartitionStyle::None) => found.push(Advice::EmulatorNoPartitionTable),
            Some(PartitionStyle::Mbr) if partition.number > 1 => {
                found.push(Advice::EmulatorPartition(partition.number));
            }
            _ => {}
        }
    }
    sorted(found)
}

/// A folder read over SMB: writes stay inside it whatever the disk holds
/// (rule 1), and no BDM rule applies (research/18 §4).
fn diagnose_smb(volume: &Volume, options: &DiagnoseOptions) -> Vec<Advice> {
    let mut found = Vec::new();
    if volume.read_only {
        found.push(Advice::ReadOnly);
    }
    found.extend(low_space(volume));
    // A disk nothing can be written to gets no folder of its own either.
    if volume.system && options.whole_volume && !volume.read_only {
        found.push(Advice::SystemRoot(
            volume.root.join(SHARE_FOLDER).display().to_string(),
        ));
    }
    if volume.fs.max_file_size().is_some() {
        found.push(Advice::LargeGamesAsUl);
    }
    found.push(Advice::Smb {
        smb2: options.profile == LoaderProfile::RiptOpl,
    });
    sorted(found)
}

fn diagnose_ps2(volume: &Volume, options: &DiagnoseOptions) -> Vec<Advice> {
    let mut found = Vec::new();
    let profile = options.profile;
    if volume.system {
        found.push(Advice::SystemVolume);
    }
    if volume.read_only {
        found.push(Advice::ReadOnly);
    }
    found.extend(low_space(volume));
    match volume.fs {
        FsKind::Fat32 | FsKind::Fat16 => {
            found.push(Advice::LargeGamesAsUl);
            if profile.reads_exfat() && !options.boot_from_device {
                found.push(Advice::ExfatRecommended);
            }
            if let Some(size) = volume.cluster_size {
                if size < 16 * 1024 {
                    found.push(Advice::ClusterSmall(size));
                } else if size > 32 * 1024 {
                    found.push(Advice::ClusterTooLarge(size));
                }
            }
        }
        FsKind::ExFat => {
            if !profile.reads_exfat() {
                found.push(Advice::ExfatNeedsOpl12);
            }
            if options.boot_from_device {
                found.push(Advice::FatNeededToBoot);
            }
            if let Some(size) = volume.cluster_size
                && size >= 512 * 1024
            {
                found.push(Advice::ClusterTooLarge(size));
            }
        }
        fs @ (FsKind::Ntfs | FsKind::Other) => {
            if volume.is_external() {
                found.push(Advice::FsUnreadable(fs));
            }
        }
    }
    if let Some(size) = volume.sector_size
        && size != 512
    {
        found.push(Advice::SectorSize(size));
    }
    if let Some(partition) = volume.partition {
        match partition.style {
            Some(PartitionStyle::Gpt) if !profile.reads_exfat() || options.boot_from_device => {
                found.push(Advice::GptUnsupported);
            }
            Some(PartitionStyle::None) if volume.is_external() => {
                found.push(Advice::NoPartitionTable);
            }
            _ => {}
        }
        if partition.number > 0 && !partition.offset.is_multiple_of(MIB) {
            found.push(Advice::Misaligned(partition.offset));
        }
    }
    if !volume.is_external() && !volume.system {
        found.push(Advice::NotExternal);
    }
    sorted(found)
}

fn sorted(mut found: Vec<Advice>) -> Vec<Advice> {
    found.sort_by_key(|a| std::cmp::Reverse(a.level()));
    found
}

/// Counts macOS AppleDouble files (`._*`) where OPL trips over them:
/// `CD/`, `DVD/`, `LNG/` and the app folders in `APPS/` (research/06 §1.10).
pub fn count_apple_double(root: &Path) -> usize {
    let count_in = |dir: &Path| -> usize {
        fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.file_name().to_string_lossy().starts_with("._"))
                    .count()
            })
            .unwrap_or(0)
    };
    let mut total: usize = ["CD", "DVD", "LNG", "APPS"]
        .iter()
        .map(|name| count_in(&root.join(name)))
        .sum();
    if let Ok(apps) = fs::read_dir(root.join("APPS")) {
        total += apps
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .map(|e| count_in(&e.path()))
            .sum::<usize>();
    }
    total
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::volume::{Bus, PartitionInfo, VolumeKind};

    fn stick(fs: FsKind, cluster: u32) -> Volume {
        Volume {
            root: PathBuf::from("E:\\"),
            label: None,
            fs,
            fs_name: fs.to_string(),
            total_bytes: 8 << 30,
            free_bytes: 8 << 30,
            cluster_size: Some(cluster),
            sector_size: Some(512),
            serial: Some("7E1CD2C5".into()),
            kind: VolumeKind::Removable,
            bus: Some(Bus::Usb),
            model: None,
            system: false,
            read_only: false,
            partition: Some(PartitionInfo {
                style: Some(PartitionStyle::Mbr),
                number: 1,
                offset: MIB,
                mbr_type: Some(0x0C),
            }),
        }
    }

    fn codes(volume: &Volume, options: DiagnoseOptions) -> Vec<&'static str> {
        diagnose(volume, &options)
            .iter()
            .map(Advice::code)
            .collect()
    }

    const OPL12: DiagnoseOptions = DiagnoseOptions {
        profile: LoaderProfile::Opl120Beta,
        boot_from_device: false,
        mode: DeviceMode::Ps2,
        whole_volume: true,
    };
    const OPL11: DiagnoseOptions = DiagnoseOptions {
        profile: LoaderProfile::Opl110,
        ..OPL12
    };
    const SMB: DiagnoseOptions = DiagnoseOptions {
        mode: DeviceMode::Smb,
        whole_volume: false,
        ..OPL12
    };
    const EMULATOR: DiagnoseOptions = DiagnoseOptions {
        mode: DeviceMode::Emulator,
        ..OPL12
    };

    #[test]
    fn an_emulator_s_sd_follows_its_firmware() {
        // The factory card (research/19 §2): MBR, partition 1, exFAT with
        // 32 KiB clusters. Nothing about clusters, exFAT or fragments.
        let factory = stick(FsKind::ExFat, 32 * 1024);
        assert!(codes(&factory, EMULATOR).is_empty());
        assert!(codes(&stick(FsKind::Fat32, 4096), EMULATOR).is_empty());
        // The profile does not matter: the firmware reads the file system.
        let opl11 = DiagnoseOptions {
            profile: LoaderProfile::Opl110,
            ..EMULATOR
        };
        assert!(codes(&factory, opl11).is_empty());
        let with = |style, number| {
            let mut volume = stick(FsKind::ExFat, 128 * 1024);
            volume.partition = Some(PartitionInfo {
                style: Some(style),
                number,
                offset: 16 * MIB,
                mbr_type: None,
            });
            volume
        };
        assert_eq!(
            codes(&with(PartitionStyle::Gpt, 1), EMULATOR),
            ["emulatorGpt"]
        );
        assert_eq!(
            codes(&with(PartitionStyle::None, 0), EMULATOR),
            ["emulatorNoPartitionTable"]
        );
        assert_eq!(
            codes(&with(PartitionStyle::Mbr, 2), EMULATOR),
            ["emulatorPartition"]
        );
        assert!(codes(&with(PartitionStyle::Mbr, 1), EMULATOR).is_empty());
        let mut ntfs = stick(FsKind::Ntfs, 4096);
        ntfs.sector_size = Some(4096);
        assert_eq!(codes(&ntfs, EMULATOR), ["emulatorFs"]);
        let mut read_only = factory.clone();
        read_only.read_only = true;
        assert_eq!(codes(&read_only, EMULATOR), ["readOnly"]);
        // A copy of the SD on the computer: no rule of the SD applies.
        let mut copy = stick(FsKind::Ntfs, 4096);
        copy.kind = VolumeKind::Fixed;
        copy.bus = Some(Bus::Nvme);
        copy.system = true;
        copy.total_bytes = 2 << 40;
        copy.free_bytes = 1 << 40;
        assert_eq!(codes(&copy, EMULATOR), ["emulatorCopy"]);
        // Little free space shows here as everywhere.
        let mut full = factory;
        full.free_bytes = 1 << 30;
        assert_eq!(codes(&full, EMULATOR), ["lowSpace"]);
    }

    #[test]
    fn fat32_stick() {
        // The owner's test stick: FAT32 with 4 KiB clusters.
        assert_eq!(
            codes(&stick(FsKind::Fat32, 4096), OPL12),
            ["largeGamesAsUl", "exfatRecommended", "clusterSmall"]
        );
        assert_eq!(
            codes(&stick(FsKind::Fat32, 32 * 1024), OPL11),
            ["largeGamesAsUl"]
        );
    }

    #[test]
    fn exfat_depends_on_profile_and_boot() {
        assert!(codes(&stick(FsKind::ExFat, 128 * 1024), OPL12).is_empty());
        assert_eq!(
            codes(&stick(FsKind::ExFat, 128 * 1024), OPL11),
            ["exfatNeedsOpl12"]
        );
        let boot = DiagnoseOptions {
            boot_from_device: true,
            ..OPL12
        };
        assert_eq!(
            codes(&stick(FsKind::ExFat, 512 * 1024), boot),
            ["fatNeededToBoot", "clusterTooLarge"]
        );
    }

    #[test]
    fn errors_come_first() {
        let mut volume = stick(FsKind::Ntfs, 4096);
        volume.sector_size = Some(4096);
        assert_eq!(codes(&volume, OPL12), ["fsUnreadable", "sectorSize"]);
        assert!(diagnose(&volume, &OPL12)[0].level() == Level::Error);

        let mut gpt = stick(FsKind::ExFat, 128 * 1024);
        gpt.partition = Some(PartitionInfo {
            style: Some(PartitionStyle::Gpt),
            number: 1,
            offset: 17 * 1024,
            mbr_type: None,
        });
        assert_eq!(
            codes(&gpt, OPL11),
            ["exfatNeedsOpl12", "gptUnsupported", "misaligned"]
        );
    }

    #[test]
    fn shares_and_internal_disks_plugged_into_the_ps2() {
        // Switched to the console mode by hand: the BDM rules apply.
        let mut share = stick(FsKind::Ntfs, 4096);
        share.kind = VolumeKind::Remote;
        share.bus = None;
        assert_eq!(codes(&share, OPL12), ["notExternal"]);

        let mut internal = stick(FsKind::Ntfs, 4096);
        internal.kind = VolumeKind::Fixed;
        internal.bus = Some(Bus::Nvme);
        assert_eq!(codes(&internal, OPL12), ["notExternal"]);
        internal.system = true;
        assert_eq!(codes(&internal, OPL12), ["systemVolume"]);
    }

    fn system_disk() -> Volume {
        let mut volume = stick(FsKind::Ntfs, 4096);
        volume.root = PathBuf::from("C:\\");
        volume.kind = VolumeKind::Fixed;
        volume.bus = Some(Bus::Nvme);
        volume.system = true;
        volume.sector_size = Some(4096);
        volume.partition = Some(PartitionInfo {
            style: Some(PartitionStyle::Gpt),
            number: 3,
            offset: 17 * 1024,
            mbr_type: None,
        });
        volume
    }

    #[test]
    fn a_folder_for_smb_on_the_system_disk_suits_opl() {
        // No system disk error, no file system, sector or partition rules.
        let volume = system_disk();
        assert_eq!(codes(&volume, SMB), ["smbRules"]);
        assert!(
            diagnose(&volume, &SMB)
                .iter()
                .all(|a| a.level() == Level::Info)
        );
        let riptopl = DiagnoseOptions {
            profile: LoaderProfile::RiptOpl,
            ..SMB
        };
        assert_eq!(codes(&volume, riptopl), ["smbRulesSmb2"]);
    }

    #[test]
    fn emulator_notes_are_information() {
        // The owner cannot change them in YAOPLM and the SD works as it is
        // (release pass 1.8): no "Has notes" verdict for them.
        assert_eq!(Advice::EmulatorCardSize(64).level(), Level::Info);
        assert_eq!(
            Advice::EmulatorChannelName {
                variant: "PS2".into(),
                channel: 3,
                name: "FMCB 1.965".into(),
                holds: "FMCB 1.963".into(),
            }
            .level(),
            Level::Info
        );
    }

    #[test]
    fn smb_rules_by_the_folder() {
        // FAT32 on the computer still holds files up to 4 GiB.
        let mut fat = stick(FsKind::Fat32, 4096);
        fat.kind = VolumeKind::Fixed;
        fat.bus = Some(Bus::Sata);
        assert_eq!(codes(&fat, SMB), ["largeGamesAsUl", "smbRules"]);
        let mut exfat = fat.clone();
        exfat.fs = FsKind::ExFat;
        let opl11 = DiagnoseOptions {
            profile: LoaderProfile::Opl110,
            ..SMB
        };
        assert_eq!(codes(&exfat, opl11), ["smbRules"]);

        let mut read_only = system_disk();
        read_only.read_only = true;
        let found = diagnose(&read_only, &SMB);
        assert_eq!(found[0], Advice::ReadOnly);
        assert_eq!(found[0].level(), Level::Error);
        // A read-only system disk (macOS's sealed volume) is not offered a
        // folder of its own.
        let whole = DiagnoseOptions {
            whole_volume: true,
            ..SMB
        };
        assert_eq!(codes(&read_only, whole), ["readOnly", "smbRules"]);
    }

    #[test]
    fn the_whole_system_disk_shared() {
        let whole = DiagnoseOptions {
            whole_volume: true,
            ..SMB
        };
        let found = diagnose(&system_disk(), &whole);
        assert_eq!(
            found.iter().map(Advice::code).collect::<Vec<_>>(),
            ["systemRoot", "smbRules"]
        );
        assert_eq!(found[0].level(), Level::Warning);
        assert_eq!(
            found[0].phrase().english(),
            format!(
                "The whole system disk is open: for sharing over the network, a separate folder such as {} is better",
                PathBuf::from("C:\\").join("PS2").display()
            )
        );
        // Only the system disk: another disk's root is an ordinary share.
        let mut data = system_disk();
        data.system = false;
        assert_eq!(codes(&data, whole), ["smbRules"]);
    }

    #[test]
    fn little_free_space_in_either_mode() {
        let gib: u64 = 1 << 30;
        let mut volume = stick(FsKind::ExFat, 128 * 1024);
        // The owner's sticks and disks on 2026-10-25.
        for (free, total, low) in [
            (3 * gib + gib / 3, 7 * gib + gib / 2, true), // E: 3.3 of 7.5 GiB
            (57 * gib, 57 * gib + gib / 2, false),        // F:, empty
            (13 * gib, 476 * gib, true),                  // C:, 2.7 % free
            (124 * gib, 466 * gib, false),                // D:
        ] {
            volume.free_bytes = free;
            volume.total_bytes = total;
            for options in [OPL12, SMB] {
                assert_eq!(
                    codes(&volume, options).contains(&"lowSpace"),
                    low,
                    "{free} of {total}"
                );
            }
        }
        volume.free_bytes = 2 * gib;
        let advice = Advice::LowSpace {
            free: volume.free_bytes,
            total: volume.total_bytes,
        };
        assert_eq!(advice.level(), Level::Warning);
        assert_eq!(
            advice.phrase().english(),
            "Little free space left: 2.0 GiB of 466.0 GiB"
        );
        // Nothing more can be written to a read-only volume anyway.
        volume.read_only = true;
        assert!(!codes(&volume, OPL12).contains(&"lowSpace"));
    }

    #[test]
    fn apple_double_files() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "CD/._Game.iso",
            "LNG/._lang_Russian.lng",
            "APPS/WLE/._WLE.ELF",
            "ART/._x.png",
        ] {
            let path = dir.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"").unwrap();
        }
        assert_eq!(count_apple_double(dir.path()), 3);
    }
}

#[cfg(test)]
mod phrase_tests {
    use super::*;

    #[test]
    fn every_advice_renders() {
        let all = [
            Advice::SystemVolume,
            Advice::ReadOnly,
            Advice::FsUnreadable(FsKind::Ntfs),
            Advice::ExfatNeedsOpl12,
            Advice::FatNeededToBoot,
            Advice::LargeGamesAsUl,
            Advice::ExfatRecommended,
            Advice::Smb { smb2: false },
            Advice::Smb { smb2: true },
            Advice::SystemRoot(r"C:\PS2".into()),
            Advice::LowSpace {
                free: 3 << 30,
                total: 8 << 30,
            },
            Advice::SectorSize(4096),
            Advice::ClusterTooLarge(256 * 1024),
            Advice::ClusterSmall(4 * 1024),
            Advice::GptUnsupported,
            Advice::NoPartitionTable,
            Advice::Misaligned(32_256),
            Advice::NotExternal,
            Advice::AppleDouble(3),
            Advice::EmulatorGpt,
            Advice::EmulatorNoPartitionTable,
            Advice::EmulatorPartition(2),
            Advice::EmulatorFs(FsKind::Ntfs),
            Advice::EmulatorCopy,
            Advice::EmulatorCardSize(64),
            Advice::EmulatorDamaged {
                count: 1,
                files: "MemoryCards/PS2/Card1/Card1-1.crp.0".into(),
            },
            Advice::EmulatorIniTail {
                file: ".sd2psx/settings.ini".into(),
                line: "CardSize=8".into(),
            },
            Advice::EmulatorChannelName {
                variant: "PS2".into(),
                channel: 3,
                name: "FMCB 1.965".into(),
                holds: "FMCB 1.963".into(),
            },
        ];
        for advice in &all {
            let phrase = advice.phrase();
            assert_eq!(phrase.key, format!("notices.{}", advice.code()));
            let text = phrase.english();
            assert_ne!(text, phrase.key);
            assert!(!text.contains("{{"), "{text}");
        }
    }
}
