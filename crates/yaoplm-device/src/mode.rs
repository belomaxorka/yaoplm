//! How OPL reads a device: plugged into the PS2 or over SMB (ADR-0043,
//! research/18).
//!
//! A folder on an internal disk can reach a PS2 only over the network, so
//! the device's place, not its contents, gives the mode: an empty folder
//! gets the right one at once. Users switch it for the exceptions (a USB
//! disk shared over the network).

use std::fs;
use std::path::Path;

use yaoplm_i18n::Phrase;

use crate::volume::{Volume, VolumeKind};

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
    /// serves it.
    pub fn of(volume: &Volume, server: bool) -> Self {
        if volume.is_external() {
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
            Self::ComputerDisk | Self::NetworkFolder | Self::ServerFolder => DeviceMode::Smb,
        }
    }

    /// To follow a dash after the mode (`reasons.*`).
    pub fn phrase(self) -> Phrase {
        Phrase::reason(match self {
            Self::Removable => "placeRemovable",
            Self::ComputerDisk => "placeComputerDisk",
            Self::NetworkFolder => "placeNetworkFolder",
            Self::ServerFolder => "placeServerFolder",
        })
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
            fs: FsKind::Fat32,
            fs_name: "FAT32".into(),
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
            // A USB disk, or an internal disk in a USB adapter: the OS
            // reports the USB bus, and it goes into the console.
            (VolumeKind::Fixed, Some(Bus::Usb), false, Place::Removable),
            (VolumeKind::Fixed, Some(Bus::Sd), true, Place::Removable),
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
