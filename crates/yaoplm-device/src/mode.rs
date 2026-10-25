//! How OPL reads a device: plugged into the PS2 or over SMB (ADR-0045,
//! research/18).
//!
//! A folder on an internal disk can reach a PS2 only over the network, so
//! the device's place, not its contents, gives the mode: an empty folder
//! gets the right one at once. Users switch it for the exceptions (a USB
//! disk shared over the network).

use std::fs;
use std::path::Path;

use yaoplm_i18n::Phrase;

use crate::volume::{FsKind, Volume, VolumeKind};

/// How OPL reads the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DeviceMode {
    /// Plugged into the console: BDM over USB, MX4SIO or i.LINK.
    #[default]
    Ps2,
    /// Shared from a computer and read over the network (SMB).
    Smb,
}

/// Where a device is, which gives its mode unless the user chose one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Place {
    /// A USB stick or disk, an SD card: what users plug into a PS2.
    Removable,
    /// A USB hard disk on a file system OPL cannot read over USB (NTFS,
    /// ext4, APFS): only the network brings it to the console. A stick or
    /// SD card on one stays `Removable`: those are for the console, and the
    /// doctor says to prepare them (owner's decision 2026-10-25, ADR-0045).
    UsbDisk(FsKind),
    /// A folder on an internal disk of the computer.
    ComputerDisk,
    /// A network share the OS mounted.
    NetworkFolder,
    /// A library root of `yaoplm-server` that is not removable (in Docker
    /// the bus is not visible at all).
    ServerFolder,
}

impl Place {
    /// The place of a device on `volume`; `server` when `yaoplm-server`
    /// serves it. A FAT or exFAT volume the OS cannot place (a stick passed
    /// into Docker, where `/sys` is hidden) is removable media too: computers
    /// keep their own disks on other file systems.
    pub fn of(volume: &Volume, server: bool) -> Self {
        let bdm_fs = matches!(volume.fs, FsKind::Fat32 | FsKind::Fat16 | FsKind::ExFat);
        let unplaced_stick = volume.kind == VolumeKind::Unknown && !volume.system && bdm_fs;
        if volume.is_external() && volume.kind != VolumeKind::Removable && !bdm_fs {
            Self::UsbDisk(volume.fs)
        } else if volume.is_external() || unplaced_stick {
            Self::Removable
        } else if volume.kind == VolumeKind::Remote {
            Self::NetworkFolder
        } else if server {
            Self::ServerFolder
        } else {
            Self::ComputerDisk
        }
    }

    pub fn mode(self) -> DeviceMode {
        match self {
            Self::Removable => DeviceMode::Ps2,
            Self::UsbDisk(_) | Self::ComputerDisk | Self::NetworkFolder | Self::ServerFolder => {
                DeviceMode::Smb
            }
        }
    }

    /// To follow a dash after the mode (`reasons.*`).
    pub fn phrase(self) -> Phrase {
        match self {
            Self::UsbDisk(FsKind::Other) => Phrase::reason("placeUsbDiskOther"),
            Self::UsbDisk(fs) => Phrase::reason("placeUsbDisk").with("fs", fs.to_string()),
            Self::Removable => Phrase::reason("placeRemovable"),
            Self::ComputerDisk => Phrase::reason("placeComputerDisk"),
            Self::NetworkFolder => Phrase::reason("placeNetworkFolder"),
            Self::ServerFolder => Phrase::reason("placeServerFolder"),
        }
    }
}

impl Volume {
    /// `path` is the volume's root (a drive or mount point), not a folder on
    /// it.
    pub fn is_root(&self, path: &Path) -> bool {
        let canonical = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        canonical(path) == canonical(&self.root)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::volume::{Bus, FsKind};

    fn volume(kind: VolumeKind, bus: Option<Bus>) -> Volume {
        Volume {
            root: PathBuf::from("E:\\"),
            label: None,
            fs: FsKind::Ntfs,
            fs_name: "NTFS".into(),
            total_bytes: 8 << 30,
            free_bytes: 8 << 30,
            cluster_size: Some(32 * 1024),
            sector_size: Some(512),
            serial: None,
            kind,
            bus,
            model: None,
            system: false,
            read_only: false,
            partition: None,
        }
    }

    #[test]
    fn the_place_gives_the_mode() {
        let cases = [
            (VolumeKind::Removable, None, false, Place::Removable),
            // A USB disk on NTFS: only the network brings it to the console.
            (
                VolumeKind::Fixed,
                Some(Bus::Usb),
                false,
                Place::UsbDisk(FsKind::Ntfs),
            ),
            // An NTFS stick is still for the console: it gets prepared.
            (
                VolumeKind::Removable,
                Some(Bus::Usb),
                false,
                Place::Removable,
            ),
            (VolumeKind::Removable, Some(Bus::Sd), true, Place::Removable),
            (
                VolumeKind::Fixed,
                Some(Bus::Nvme),
                false,
                Place::ComputerDisk,
            ),
            (VolumeKind::Unknown, None, false, Place::ComputerDisk),
            (VolumeKind::Remote, None, false, Place::NetworkFolder),
            (VolumeKind::Remote, None, true, Place::NetworkFolder),
            (
                VolumeKind::Fixed,
                Some(Bus::Sata),
                true,
                Place::ServerFolder,
            ),
            // Docker: no bus.
            (VolumeKind::Unknown, None, true, Place::ServerFolder),
        ];
        for (kind, bus, server, place) in cases {
            assert_eq!(
                Place::of(&volume(kind, bus), server),
                place,
                "{kind:?} {bus:?}"
            );
        }
        // A USB disk, or an internal disk in a USB adapter, on FAT32 or
        // exFAT: the OS reports the USB bus, and it goes into the console.
        let mut usb_disk = volume(VolumeKind::Fixed, Some(Bus::Usb));
        usb_disk.fs = FsKind::ExFat;
        assert_eq!(Place::of(&usb_disk, false), Place::Removable);
        usb_disk.fs = FsKind::Other;
        assert_eq!(Place::of(&usb_disk, true), Place::UsbDisk(FsKind::Other));
        assert_eq!(Place::UsbDisk(FsKind::Ntfs).mode(), DeviceMode::Smb);
        // A stick passed into Docker: no bus, but FAT32 or exFAT.
        let mut stick = volume(VolumeKind::Unknown, None);
        for fs in [FsKind::Fat32, FsKind::ExFat] {
            stick.fs = fs;
            assert_eq!(Place::of(&stick, true), Place::Removable);
        }
        stick.system = true;
        assert_eq!(Place::of(&stick, true), Place::ServerFolder);
        assert_eq!(Place::Removable.mode(), DeviceMode::Ps2);
        for place in [
            Place::ComputerDisk,
            Place::NetworkFolder,
            Place::ServerFolder,
        ] {
            assert_eq!(place.mode(), DeviceMode::Smb);
        }
    }

    #[test]
    fn every_place_renders() {
        for place in [
            Place::Removable,
            Place::UsbDisk(FsKind::Ntfs),
            Place::UsbDisk(FsKind::Other),
            Place::ComputerDisk,
            Place::NetworkFolder,
            Place::ServerFolder,
        ] {
            let text = place.phrase().english();
            assert!(
                !text.contains("{{") && !text.starts_with("reasons."),
                "{text}"
            );
        }
    }

    #[test]
    fn a_folder_is_not_the_volume_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut volume = volume(VolumeKind::Fixed, None);
        volume.root = dir.path().to_path_buf();
        assert!(volume.is_root(dir.path()));
        let inner = dir.path().join("PS2");
        fs::create_dir(&inner).unwrap();
        assert!(!volume.is_root(&inner));
    }
}
