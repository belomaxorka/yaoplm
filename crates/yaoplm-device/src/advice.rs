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

use std::fmt;
use std::fs;
use std::path::Path;

use yaoplm_formats::LoaderProfile;

use crate::volume::{FsKind, PartitionStyle, Volume, VolumeKind};

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
    /// OPL reads network shares with SMBv1 only; names up to 159 bytes and
    /// at most 10 `ul` parts.
    RemoteShare,
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
}

impl Advice {
    pub fn level(&self) -> Level {
        match self {
            Self::SystemVolume
            | Self::ReadOnly
            | Self::FsUnreadable(_)
            | Self::ExfatNeedsOpl12
            | Self::SectorSize(_)
            | Self::GptUnsupported => Level::Error,
            Self::FatNeededToBoot
            | Self::ClusterTooLarge(_)
            | Self::NoPartitionTable
            | Self::AppleDouble(_) => Level::Warning,
            Self::LargeGamesAsUl
            | Self::ExfatRecommended
            | Self::RemoteShare
            | Self::ClusterSmall(_)
            | Self::Misaligned(_)
            | Self::NotExternal => Level::Info,
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
            Self::RemoteShare => "remoteShare",
            Self::SectorSize(_) => "sectorSize",
            Self::ClusterTooLarge(_) => "clusterTooLarge",
            Self::ClusterSmall(_) => "clusterSmall",
            Self::GptUnsupported => "gptUnsupported",
            Self::NoPartitionTable => "noPartitionTable",
            Self::Misaligned(_) => "misaligned",
            Self::NotExternal => "notExternal",
            Self::AppleDouble(_) => "appleDouble",
        }
    }
}

fn kib(bytes: u32) -> u32 {
    bytes / 1024
}

impl fmt::Display for Advice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SystemVolume => write!(f, "this volume holds the operating system"),
            Self::ReadOnly => write!(f, "the volume is read-only"),
            Self::FsUnreadable(fs) => write!(
                f,
                "OPL cannot read {fs} from USB or SD; use exFAT (OPL 1.2.0 beta) or FAT32"
            ),
            Self::ExfatNeedsOpl12 => {
                write!(
                    f,
                    "OPL 1.1.0 cannot read exFAT; use FAT32 or OPL 1.2.0 beta"
                )
            }
            Self::FatNeededToBoot => write!(
                f,
                "FreeMcBoot and old uLaunchELF cannot start OPL from exFAT; this device must be FAT32 on MBR"
            ),
            Self::LargeGamesAsUl => write!(
                f,
                "FAT32 holds files up to 4 GiB: larger games are split into 1 GiB ul chunks"
            ),
            Self::ExfatRecommended => write!(
                f,
                "exFAT is recommended for OPL 1.2.0 beta: games over 4 GiB stay one image"
            ),
            Self::RemoteShare => write!(
                f,
                "network share: OPL speaks SMBv1 only, opens names up to 159 bytes and at most 10 ul parts"
            ),
            Self::SectorSize(size) => write!(
                f,
                "logical sectors are {size} bytes; OPL reads 512-byte sectors only (often a USB adapter issue)"
            ),
            Self::ClusterTooLarge(size) => write!(
                f,
                "{} KiB clusters are reported to cause problems; 32-128 KiB is safe",
                kib(*size)
            ),
            Self::ClusterSmall(size) => write!(
                f,
                "{} KiB FAT32 clusters make OPL read more of the FAT when a game starts; 32 KiB is recommended",
                kib(*size)
            ),
            Self::GptUnsupported => write!(
                f,
                "GPT partition table: OPL 1.1.0 and legacy loaders read MBR only"
            ),
            Self::NoPartitionTable => write!(
                f,
                "no partition table: some OPL 1.2.0 builds cannot mount this; MBR with one partition is safest"
            ),
            Self::Misaligned(offset) => write!(
                f,
                "the partition starts at byte {offset}, not on a 1 MiB boundary (slower writes)"
            ),
            Self::NotExternal => write!(
                f,
                "not a USB or SD device: OPL reads games from USB, MX4SIO, iLink, an internal HDD or SMB"
            ),
            Self::AppleDouble(count) => write!(
                f,
                "{count} macOS ._ files in CD, DVD, LNG or APPS slow down OPL or show up as bogus entries"
            ),
        }
    }
}

const MIB: u64 = 1 << 20;

/// Everything worth telling about `volume` for `options`, worst first.
pub fn diagnose(volume: &Volume, options: &DiagnoseOptions) -> Vec<Advice> {
    let mut found = Vec::new();
    let profile = options.profile;
    if volume.system {
        found.push(Advice::SystemVolume);
    }
    if volume.read_only {
        found.push(Advice::ReadOnly);
    }
    if volume.kind == VolumeKind::Remote {
        found.push(Advice::RemoteShare);
        return sorted(found);
    }
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
    use crate::volume::{Bus, PartitionInfo};

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
    };
    const OPL11: DiagnoseOptions = DiagnoseOptions {
        profile: LoaderProfile::Opl110,
        boot_from_device: false,
    };

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
    fn shares_and_internal_disks() {
        let mut share = stick(FsKind::Ntfs, 4096);
        share.kind = VolumeKind::Remote;
        share.bus = None;
        assert_eq!(codes(&share, OPL12), ["remoteShare"]);

        let mut internal = stick(FsKind::Ntfs, 4096);
        internal.kind = VolumeKind::Fixed;
        internal.bus = Some(Bus::Nvme);
        assert_eq!(codes(&internal, OPL12), ["notExternal"]);
        internal.system = true;
        assert_eq!(codes(&internal, OPL12), ["systemVolume"]);
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
