//! Linux: mounts from `/proc/self/mountinfo`, sizes from `statvfs`, disk
//! details from sysfs, serial numbers and labels from `/dev/disk/by-*`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::volume::{Bus, FsKind, PartitionInfo, Volume, VolumeKind};

#[derive(Debug, Clone)]
struct Mount {
    point: PathBuf,
    /// The mountinfo "root" field: `/` unless this is a bind mount.
    source_root: String,
    fs_type: String,
    source: String,
    read_only: bool,
}

const REMOTE: [&str; 5] = ["cifs", "smb3", "smbfs", "nfs", "nfs4"];

/// Undoes the octal escapes mountinfo uses for spaces, tabs and newlines.
fn unescape_octal(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let octal = bytes
            .get(i + 1..i + 4)
            .filter(|digits| bytes[i] == b'\\' && digits.iter().all(|b| (b'0'..=b'7').contains(b)))
            .and_then(|d| {
                let value = u32::from(d[0] - b'0') * 64
                    + u32::from(d[1] - b'0') * 8
                    + u32::from(d[2] - b'0');
                u8::try_from(value).ok()
            });
        if let Some(value) = octal {
            out.push(value);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Undoes the `\xHH` escapes udev uses in `/dev/disk/by-label`.
fn unescape_hex(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\'
            && bytes.get(i + 1) == Some(&b'x')
            && let Some(value) = text
                .get(i + 2..i + 4)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(value);
            i += 4;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_mountinfo(text: &str) -> Vec<Mount> {
    text.lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let left: Vec<&str> = left.split(' ').collect();
            let right: Vec<&str> = right.split(' ').collect();
            let options = left.get(5)?;
            let super_options = right.get(2).copied().unwrap_or("");
            let read_only = options.split(',').any(|o| o == "ro")
                || super_options.split(',').any(|o| o == "ro");
            Some(Mount {
                point: PathBuf::from(unescape_octal(left.get(4)?)),
                source_root: unescape_octal(left.get(3)?),
                fs_type: (*right.first()?).to_owned(),
                source: unescape_octal(right.get(1)?),
                read_only,
            })
        })
        .collect()
}

fn mounts() -> Vec<Mount> {
    fs::read_to_string("/proc/self/mountinfo")
        .map(|text| parse_mountinfo(&text))
        .unwrap_or_default()
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// The `/dev/disk/by-<kind>` name pointing at `device`.
fn by_link(kind: &str, device: &Path) -> Option<String> {
    let entries = fs::read_dir(format!("/dev/disk/by-{kind}")).ok()?;
    entries.flatten().find_map(|entry| {
        let target = fs::canonicalize(entry.path()).ok()?;
        (target == device).then(|| entry.file_name().to_string_lossy().into_owned())
    })
}

/// FAT and exFAT volume IDs look like `7E1C-D2C5`; keep them as `7E1CD2C5`.
fn serial_from_uuid(uuid: &str) -> String {
    let bytes = uuid.as_bytes();
    if bytes.len() == 9 && bytes[4] == b'-' {
        uuid.replace('-', "").to_ascii_uppercase()
    } else {
        uuid.to_ascii_uppercase()
    }
}

struct BlockInfo {
    disk: String,
    removable: bool,
    bus: Option<Bus>,
    model: Option<String>,
    sector_size: Option<u32>,
    partition: Option<PartitionInfo>,
}

fn block_info(device: &Path) -> Option<BlockInfo> {
    let name = device.file_name()?.to_str()?;
    let class = PathBuf::from("/sys/class/block").join(name);
    let number = read_trimmed(class.join("partition")).and_then(|n| n.parse::<u32>().ok());
    let disk = match number {
        Some(_) => fs::canonicalize(&class)
            .ok()?
            .parent()?
            .file_name()?
            .to_str()?
            .to_owned(),
        None => name.to_owned(),
    };
    let sys_disk = PathBuf::from("/sys/block").join(&disk);
    let real = fs::canonicalize(&sys_disk)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bus = if real.contains("/usb") {
        Some(Bus::Usb)
    } else if real.contains("/mmc") {
        Some(Bus::Sd)
    } else if real.contains("/nvme") {
        Some(Bus::Nvme)
    } else if real.contains("/ata") {
        Some(Bus::Sata)
    } else {
        None
    };
    let model = match (
        read_trimmed(sys_disk.join("device/vendor")),
        read_trimmed(sys_disk.join("device/model")),
    ) {
        (Some(vendor), Some(model)) => Some(format!("{vendor} {model}")),
        (vendor, model) => vendor.or(model),
    };
    let partition = match number {
        Some(number) => Some(PartitionInfo {
            style: None,
            number,
            offset: read_trimmed(class.join("start"))
                .and_then(|s| s.parse::<u64>().ok())
                .map_or(0, |sectors| sectors * 512),
            mbr_type: None,
        }),
        None => Some(PartitionInfo {
            style: Some(crate::volume::PartitionStyle::None),
            number: 0,
            offset: 0,
            mbr_type: None,
        }),
    };
    Some(BlockInfo {
        removable: read_trimmed(sys_disk.join("removable")).as_deref() == Some("1"),
        bus,
        model,
        sector_size: read_trimmed(sys_disk.join("queue/logical_block_size"))
            .and_then(|s| s.parse().ok()),
        partition,
        disk,
    })
}

fn is_system_mount(point: &Path) -> bool {
    let text = point.to_string_lossy();
    matches!(text.as_ref(), "/" | "/usr" | "/var" | "/home") || text.starts_with("/boot")
}

fn volume(mount: &Mount, system_disk: Option<&str>) -> io::Result<Volume> {
    let stat = rustix::fs::statvfs(&mount.point).map_err(io::Error::from)?;
    let device = mount
        .source
        .starts_with("/dev/")
        .then(|| fs::canonicalize(&mount.source).ok())
        .flatten();
    let block = device.as_deref().and_then(block_info);
    let remote = REMOTE.contains(&mount.fs_type.as_str());
    let kind = if remote {
        VolumeKind::Remote
    } else if matches!(mount.fs_type.as_str(), "iso9660" | "udf") {
        VolumeKind::Optical
    } else {
        match &block {
            Some(b) if b.removable => VolumeKind::Removable,
            Some(_) => VolumeKind::Fixed,
            None => VolumeKind::Unknown,
        }
    };
    let system = is_system_mount(&mount.point)
        || matches!((&block, system_disk), (Some(b), Some(disk)) if b.disk == disk);
    let fs_kind = match mount.fs_type.as_str() {
        // ntfs-3g mounts as fuseblk; the type cannot be told apart here.
        "fuseblk" => FsKind::Other,
        other => FsKind::from_os_name(other),
    };
    let frsize = stat.f_frsize.max(1);
    Ok(Volume {
        root: mount.point.clone(),
        label: device
            .as_deref()
            .and_then(|d| by_link("label", d))
            .map(|l| unescape_hex(&l)),
        fs: fs_kind,
        fs_name: mount.fs_type.clone(),
        total_bytes: stat.f_blocks.saturating_mul(frsize),
        free_bytes: stat.f_bavail.saturating_mul(frsize),
        cluster_size: u32::try_from(stat.f_bsize).ok().filter(|&b| b > 0),
        sector_size: block.as_ref().and_then(|b| b.sector_size),
        serial: device
            .as_deref()
            .and_then(|d| by_link("uuid", d))
            .map(|u| serial_from_uuid(&u)),
        kind,
        bus: block.as_ref().and_then(|b| b.bus),
        model: block.as_ref().and_then(|b| b.model.clone()),
        system,
        read_only: mount.read_only,
        partition: block.as_ref().and_then(|b| b.partition),
    })
}

fn system_disk(mounts: &[Mount]) -> Option<String> {
    let root = mounts.iter().rev().find(|m| m.point == Path::new("/"))?;
    let device = fs::canonicalize(&root.source).ok()?;
    block_info(&device).map(|b| b.disk)
}

pub fn list_volumes() -> Vec<Volume> {
    let mounts = mounts();
    let system = system_disk(&mounts);
    let mut seen = std::collections::HashSet::new();
    mounts
        .iter()
        .filter(|m| {
            let real = (m.source.starts_with("/dev/") && m.fs_type != "squashfs")
                || REMOTE.contains(&m.fs_type.as_str());
            real && m.source_root == "/" && seen.insert(m.source.clone())
        })
        .filter_map(|m| volume(m, system.as_deref()).ok())
        .collect()
}

pub fn volume_for_path(path: &Path) -> io::Result<Volume> {
    let path = fs::canonicalize(path)?;
    let mounts = mounts();
    let mount = mounts
        .iter()
        .filter(|m| path.starts_with(&m.point))
        .max_by_key(|m| m.point.components().count())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no mount point holds this path"))?;
    volume(mount, system_disk(&mounts).as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mountinfo_lines() {
        let text = "36 35 98:0 / /media/user/OPL\\040STICK rw,nosuid master:1 - vfat /dev/sdb1 rw,fmask=0022\n\
                    37 35 0:5 / /proc rw - proc proc rw\n";
        let mounts = parse_mountinfo(text);
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].point, PathBuf::from("/media/user/OPL STICK"));
        assert_eq!(mounts[0].fs_type, "vfat");
        assert_eq!(mounts[0].source, "/dev/sdb1");
        assert!(!mounts[0].read_only);
    }

    #[test]
    fn escapes_and_serials() {
        assert_eq!(unescape_hex("OPL\\x20GAMES"), "OPL GAMES");
        assert_eq!(serial_from_uuid("7e1c-d2c5"), "7E1CD2C5");
    }
}
