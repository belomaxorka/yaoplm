//! Mounted volumes as the OS reports them (DEV-1).
//!
//! What OPL cares about (research/02 §2, research/06 §1): the file system
//! (FAT32 or exFAT on BDM devices), 512-byte logical sectors, the cluster
//! size, the partition table and whether the device is a network share. The
//! volume serial number identifies a stick between the plan and the write.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::sys;

/// A file system family, as far as OPL is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsKind {
    Fat32,
    /// FAT12/FAT16 (Windows reports both as "FAT").
    Fat16,
    ExFat,
    Ntfs,
    /// Anything else: ext4, APFS, tmpfs, a network file system...
    Other,
}

impl FsKind {
    /// Classifies the name the OS reports (`FAT32`, `vfat`, `msdos`, ...).
    pub fn from_os_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "fat32" | "vfat" | "msdos" => Self::Fat32,
            "fat" | "fat16" | "fat12" => Self::Fat16,
            "exfat" => Self::ExFat,
            "ntfs" | "ntfs3" => Self::Ntfs,
            _ => Self::Other,
        }
    }

    /// FAT stores file sizes in 32 bits: 4 GiB minus one byte.
    pub fn max_file_size(self) -> Option<u64> {
        match self {
            Self::Fat32 | Self::Fat16 => Some(u64::from(u32::MAX)),
            Self::ExFat | Self::Ntfs | Self::Other => None,
        }
    }
}

impl fmt::Display for FsKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fat32 => "FAT32",
            Self::Fat16 => "FAT16",
            Self::ExFat => "exFAT",
            Self::Ntfs => "NTFS",
            Self::Other => "other",
        })
    }
}

/// How the OS classifies the drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VolumeKind {
    /// Removable media: USB sticks, SD cards.
    Removable,
    /// Fixed disks, including most USB hard drives.
    Fixed,
    /// A network share (SMB, NFS).
    Remote,
    Optical,
    Unknown,
}

/// The bus the disk hangs off, when the OS tells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    Usb,
    /// SD or MMC card reader.
    Sd,
    Ieee1394,
    Sata,
    Nvme,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartitionStyle {
    Mbr,
    Gpt,
    /// A file system on the whole device (superfloppy).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PartitionInfo {
    /// `None` when the OS does not say (Linux without udev data).
    pub style: Option<PartitionStyle>,
    /// 1-based; 0 for a superfloppy.
    pub number: u32,
    /// Byte offset of the partition on the disk.
    pub offset: u64,
    /// MBR partition type byte (`0x0C` FAT32 LBA, `0x07` exFAT/NTFS).
    pub mbr_type: Option<u8>,
}

/// A mounted volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Volume {
    /// Mount point or drive root, e.g. `E:\` or `/media/user/OPL`.
    pub root: PathBuf,
    pub label: Option<String>,
    pub fs: FsKind,
    /// The file system name exactly as the OS reports it.
    pub fs_name: String,
    pub total_bytes: u64,
    /// Space the current user may still write.
    pub free_bytes: u64,
    pub cluster_size: Option<u32>,
    /// Logical sector size. OPL only reads 512-byte sectors.
    pub sector_size: Option<u32>,
    /// Volume serial number in upper-case hex, e.g. `7E1CD2C5` (FAT32 and
    /// exFAT store 32 bits; Windows shows it as `7E1C-D2C5`).
    pub serial: Option<String>,
    pub kind: VolumeKind,
    pub bus: Option<Bus>,
    /// Vendor and product of the disk, when the OS tells.
    pub model: Option<String>,
    /// Holds the running OS: never a target.
    pub system: bool,
    pub read_only: bool,
    pub partition: Option<PartitionInfo>,
}

impl Volume {
    /// A USB or SD device, or removable media: what users plug into a PS2.
    pub fn is_external(&self) -> bool {
        self.kind == VolumeKind::Removable
            || matches!(self.bus, Some(Bus::Usb | Bus::Sd | Bus::Ieee1394))
    }

    /// What identifies this volume between a plan and its execution: a
    /// re-plugged or swapped stick changes at least one of these.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.serial.as_deref().unwrap_or("-"),
            self.fs_name,
            self.total_bytes,
            self.label.as_deref().unwrap_or("")
        )
    }
}

/// Every mounted volume the OS lists, sorted by root. Pseudo file systems
/// (procfs, tmpfs and the like) are left out.
pub fn list_volumes() -> Vec<Volume> {
    let mut volumes = sys::list_volumes();
    volumes.sort_by(|a, b| a.root.cmp(&b.root));
    volumes
}

/// The volume holding `path` (which must exist).
pub fn volume_for_path(path: &Path) -> io::Result<Volume> {
    sys::volume_for_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fs_names() {
        assert_eq!(FsKind::from_os_name("FAT32"), FsKind::Fat32);
        assert_eq!(FsKind::from_os_name("vfat"), FsKind::Fat32);
        assert_eq!(FsKind::from_os_name("msdos"), FsKind::Fat32);
        assert_eq!(FsKind::from_os_name("FAT"), FsKind::Fat16);
        assert_eq!(FsKind::from_os_name("exFAT"), FsKind::ExFat);
        assert_eq!(FsKind::from_os_name("ntfs3"), FsKind::Ntfs);
        assert_eq!(FsKind::from_os_name("ext4"), FsKind::Other);
        assert_eq!(FsKind::Fat32.max_file_size(), Some(4_294_967_295));
        assert_eq!(FsKind::ExFat.max_file_size(), None);
    }

    #[test]
    fn lists_and_finds_volumes() {
        // Smoke test on the real machine: read-only queries.
        let volumes = list_volumes();
        assert!(!volumes.is_empty(), "no volumes listed");
        let dir = tempfile::tempdir().unwrap();
        let volume = volume_for_path(dir.path()).unwrap();
        assert!(volume.total_bytes > 0, "{volume:?}");
        assert!(volume.free_bytes <= volume.total_bytes, "{volume:?}");
    }
}
