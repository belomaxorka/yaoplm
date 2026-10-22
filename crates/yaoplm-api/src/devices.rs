//! Wire types for volumes and the storage doctor (DEV-1, DEV-2).

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_device::advice::{self, Advice, DiagnoseOptions};
use yaoplm_device::{Bus, FsKind, PartitionStyle, Volume, VolumeKind};

use crate::paths::display_path;
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
    }
}

impl DeviceInfo {
    pub fn new(volume: &Volume, options: &DiagnoseOptions) -> Self {
        Self {
            root: display_path(&volume.root),
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
            advice: advice::diagnose(volume, options)
                .iter()
                .map(notice)
                .collect(),
            apple_double: None,
            unfinished: Vec::new(),
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
