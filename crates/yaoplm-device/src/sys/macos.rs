//! macOS: mount points from `statfs`, the volumes under `/Volumes`.
//!
//! Bus, partition and serial details need DiskArbitration or `diskutil`;
//! they are left empty for now.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::volume::{FsKind, Volume, VolumeKind};

const MNT_RDONLY: u32 = 0x0000_0001;
const MNT_LOCAL: u32 = 0x0000_1000;
const MNT_ROOTFS: u32 = 0x0000_4000;

fn c_text(field: &[std::ffi::c_char]) -> String {
    let bytes: Vec<u8> = field
        .iter()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

fn volume(path: &Path) -> io::Result<Volume> {
    let stat = rustix::fs::statfs(path).map_err(io::Error::from)?;
    let root = PathBuf::from(c_text(&stat.f_mntonname));
    let fs_name = c_text(&stat.f_fstypename);
    let flags = stat.f_flags;
    let bsize = u64::from(stat.f_bsize);
    let system =
        flags & MNT_ROOTFS != 0 || root == Path::new("/") || root.starts_with("/System/Volumes");
    // Without DiskArbitration the bus is unknown. A FAT, exFAT or NTFS
    // volume under `/Volumes` is external media (a stick, an SD card, a USB
    // disk), which the device's mode needs (ADR-0047); APFS and HFS+ there
    // are the Mac's own disks, Time Machine and images.
    let fs = FsKind::from_os_name(&fs_name);
    let kind = if flags & MNT_LOCAL == 0 {
        VolumeKind::Remote
    } else if system {
        VolumeKind::Fixed
    } else if root.starts_with("/Volumes")
        && matches!(
            fs,
            FsKind::Fat32 | FsKind::Fat16 | FsKind::ExFat | FsKind::Ntfs
        )
    {
        VolumeKind::Removable
    } else {
        VolumeKind::Unknown
    };
    let label = root
        .strip_prefix("/Volumes")
        .ok()
        .and_then(|rest| rest.to_str())
        .filter(|rest| !rest.is_empty())
        .map(str::to_owned);
    Ok(Volume {
        label,
        fs,
        fs_name,
        total_bytes: stat.f_blocks.saturating_mul(bsize),
        free_bytes: stat.f_bavail.saturating_mul(bsize),
        cluster_size: Some(stat.f_bsize).filter(|&b| b > 0),
        sector_size: None,
        serial: None,
        kind,
        bus: None,
        model: None,
        system,
        read_only: flags & MNT_RDONLY != 0,
        partition: None,
        root,
    })
}

pub fn list_volumes() -> Vec<Volume> {
    let mut roots = vec![PathBuf::from("/")];
    if let Ok(entries) = fs::read_dir("/Volumes") {
        roots.extend(entries.flatten().map(|e| e.path()));
    }
    let mut volumes: Vec<Volume> = roots.iter().filter_map(|r| volume(r).ok()).collect();
    // "/Volumes/Macintosh HD" links back to "/".
    volumes.sort_by(|a, b| a.root.cmp(&b.root));
    volumes.dedup_by(|a, b| a.root == b.root);
    volumes
}

pub fn volume_for_path(path: &Path) -> io::Result<Volume> {
    volume(path)
}
