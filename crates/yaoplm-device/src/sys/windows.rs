//! Windows: volume queries through kernel32, preallocation and the
//! fragment count of a file.
//!
//! One of the modules with `unsafe` (ADR-0019, ADR-0037): thin wrappers
//! around documented Win32 calls, with every buffer owned and sized on the
//! Rust side. None needs administrator rights. The volume device is opened with
//! access 0, which still allows `IOCTL_STORAGE_QUERY_PROPERTY` and
//! `IOCTL_DISK_GET_PARTITION_INFO_EX` (research/06 §2.1).
#![allow(unsafe_code)]

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::iter;
use std::mem::size_of;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::ptr;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    BusType1394, BusTypeAta, BusTypeMmc, BusTypeNvme, BusTypeSata, BusTypeSd, BusTypeUsb,
    CreateFileW, FILE_ALLOCATION_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FileAllocationInfo, GetDiskFreeSpaceExW, GetDiskFreeSpaceW, GetDriveTypeW,
    GetLogicalDriveStringsW, GetVolumeInformationW, GetVolumePathNameW, OPEN_EXISTING,
    STORAGE_BUS_TYPE, SetFileInformationByHandle,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::{
    FSCTL_GET_RETRIEVAL_POINTERS, IOCTL_DISK_GET_PARTITION_INFO_EX, IOCTL_STORAGE_QUERY_PROPERTY,
    PARTITION_INFORMATION_EX, PARTITION_STYLE_GPT, PARTITION_STYLE_MBR, PropertyStandardQuery,
    STARTING_VCN_INPUT_BUFFER, STORAGE_DEVICE_DESCRIPTOR, STORAGE_PROPERTY_QUERY,
    StorageDeviceProperty,
};
use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
use windows_sys::Win32::System::SystemServices::FILE_READ_ONLY_VOLUME;
use windows_sys::Win32::System::WindowsProgramming::{
    DRIVE_CDROM, DRIVE_FIXED, DRIVE_REMOTE, DRIVE_REMOVABLE,
};

use super::runs::Runs;
use crate::volume::{Bus, FsKind, PartitionInfo, PartitionStyle, Volume, VolumeKind};

/// MAX_PATH + 1: the documented size for volume and file-system names.
const NAME_BUF: usize = 261;
const PATH_BUF: usize = 32_768;

fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
    text.encode_wide().chain(iter::once(0)).collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

/// `\\?\E:\` → `E:\`, `\\?\UNC\srv\share\` → `\\srv\share\`: the root
/// functions document the plain forms.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Owns a handle from `CreateFileW`.
struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful CreateFileW and is
        // closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Opens `\\.\E:` for queries only (access 0).
fn open_volume_device(root: &Path) -> Option<Handle> {
    let text = root.to_string_lossy();
    let bytes = text.as_bytes();
    let is_drive_root = bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\';
    if !is_drive_root {
        return None;
    }
    let device = wide(std::ffi::OsStr::new(&format!(r"\\.\{}:", &text[..1])));
    // SAFETY: `device` is NUL-terminated and outlives the call; the other
    // arguments are plain values or null as documented.
    let handle = unsafe {
        CreateFileW(
            device.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    (handle != INVALID_HANDLE_VALUE).then_some(Handle(handle))
}

/// Bus and model from `IOCTL_STORAGE_QUERY_PROPERTY`.
fn storage_descriptor(device: &Handle) -> Option<(Option<Bus>, Option<String>)> {
    let query = STORAGE_PROPERTY_QUERY {
        PropertyId: StorageDeviceProperty,
        QueryType: PropertyStandardQuery,
        AdditionalParameters: [0],
    };
    // u64 elements keep the descriptor suitably aligned.
    let mut out = [0u64; 128];
    let out_len = size_of::<[u64; 128]>() as u32;
    let mut returned = 0u32;
    // SAFETY: input and output buffers are valid for the sizes passed; the
    // call is synchronous (no OVERLAPPED).
    let ok = unsafe {
        DeviceIoControl(
            device.0,
            IOCTL_STORAGE_QUERY_PROPERTY,
            (&raw const query).cast(),
            size_of::<STORAGE_PROPERTY_QUERY>() as u32,
            out.as_mut_ptr().cast(),
            out_len,
            &mut returned,
            ptr::null_mut(),
        )
    };
    let returned = returned as usize;
    if ok == 0 || returned < size_of::<STORAGE_DEVICE_DESCRIPTOR>() {
        return None;
    }
    // SAFETY: the buffer holds at least one descriptor and is 8-aligned.
    let descriptor: STORAGE_DEVICE_DESCRIPTOR =
        unsafe { ptr::read(out.as_ptr().cast::<STORAGE_DEVICE_DESCRIPTOR>()) };
    let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_le_bytes()).collect();
    let bytes = &bytes[..returned.min(bytes.len())];
    let text_at = |offset: u32| -> Option<String> {
        let start = usize::try_from(offset).ok().filter(|&o| o != 0)?;
        let tail = bytes.get(start..)?;
        let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        let text = String::from_utf8_lossy(&tail[..end]).trim().to_owned();
        (!text.is_empty()).then_some(text)
    };
    let model = match (
        text_at(descriptor.VendorIdOffset),
        text_at(descriptor.ProductIdOffset),
    ) {
        (Some(vendor), Some(product)) => Some(format!("{vendor} {product}")),
        (vendor, product) => vendor.or(product),
    };
    Some((bus(descriptor.BusType), model))
}

fn bus(kind: STORAGE_BUS_TYPE) -> Option<Bus> {
    let table = [
        (BusTypeUsb, Bus::Usb),
        (BusTypeSd, Bus::Sd),
        (BusTypeMmc, Bus::Sd),
        (BusType1394, Bus::Ieee1394),
        (BusTypeSata, Bus::Sata),
        (BusTypeAta, Bus::Sata),
        (BusTypeNvme, Bus::Nvme),
    ];
    match table.iter().find(|(value, _)| *value == kind) {
        Some((_, bus)) => Some(*bus),
        None if kind == 0 => None,
        None => Some(Bus::Other),
    }
}

fn partition_info(device: &Handle) -> Option<PartitionInfo> {
    let mut info = PARTITION_INFORMATION_EX::default();
    let mut returned = 0u32;
    // SAFETY: `info` is a valid output buffer of the size passed.
    let ok = unsafe {
        DeviceIoControl(
            device.0,
            IOCTL_DISK_GET_PARTITION_INFO_EX,
            ptr::null(),
            0,
            (&raw mut info).cast(),
            size_of::<PARTITION_INFORMATION_EX>() as u32,
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let number = info.PartitionNumber;
    let (style, mbr_type) = match info.PartitionStyle {
        // SAFETY: the MBR member is the active one for this style.
        PARTITION_STYLE_MBR if number > 0 => (
            PartitionStyle::Mbr,
            Some(unsafe { info.Anonymous.Mbr.PartitionType }),
        ),
        PARTITION_STYLE_GPT => (PartitionStyle::Gpt, None),
        _ => (PartitionStyle::None, None),
    };
    Some(PartitionInfo {
        style: Some(style),
        number,
        offset: u64::try_from(info.StartingOffset).unwrap_or(0),
        mbr_type,
    })
}

fn windows_root() -> Option<PathBuf> {
    let mut buf = [0u16; NAME_BUF];
    // SAFETY: the buffer is valid for its length.
    let len = unsafe { GetWindowsDirectoryW(buf.as_mut_ptr(), buf.len() as u32) };
    let dir = PathBuf::from(from_wide(&buf[..(len as usize).min(buf.len())]));
    dir.ancestors().last().map(Path::to_path_buf)
}

fn query(root: &Path) -> io::Result<Volume> {
    let root_w = wide(root.as_os_str());
    // SAFETY: `root_w` is NUL-terminated.
    let drive_type = unsafe { GetDriveTypeW(root_w.as_ptr()) };

    let mut label = [0u16; NAME_BUF];
    let mut fs_name = [0u16; NAME_BUF];
    let (mut serial, mut max_component, mut flags) = (0u32, 0u32, 0u32);
    // SAFETY: all buffers are valid for the lengths passed.
    let ok = unsafe {
        GetVolumeInformationW(
            root_w.as_ptr(),
            label.as_mut_ptr(),
            label.len() as u32,
            &mut serial,
            &mut max_component,
            &mut flags,
            fs_name.as_mut_ptr(),
            fs_name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    let (mut available, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: plain out-parameters.
    let ok = unsafe {
        GetDiskFreeSpaceExW(root_w.as_ptr(), &mut available, &mut total, &mut total_free)
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    let (mut per_cluster, mut sector, mut free_clusters, mut clusters) = (0u32, 0u32, 0u32, 0u32);
    // SAFETY: plain out-parameters.
    let ok = unsafe {
        GetDiskFreeSpaceW(
            root_w.as_ptr(),
            &mut per_cluster,
            &mut sector,
            &mut free_clusters,
            &mut clusters,
        )
    };
    let (cluster_size, sector_size) = if ok != 0 && sector > 0 {
        (per_cluster.checked_mul(sector), Some(sector))
    } else {
        (None, None)
    };

    let kind = match drive_type {
        DRIVE_REMOVABLE => VolumeKind::Removable,
        DRIVE_FIXED => VolumeKind::Fixed,
        DRIVE_REMOTE => VolumeKind::Remote,
        DRIVE_CDROM => VolumeKind::Optical,
        _ => VolumeKind::Unknown,
    };
    let device = (kind != VolumeKind::Remote)
        .then(|| open_volume_device(root))
        .flatten();
    let (bus, model) = device
        .as_ref()
        .and_then(storage_descriptor)
        .unwrap_or((None, None));
    let partition = device.as_ref().and_then(partition_info);
    let system = windows_root().is_some_and(|win| {
        win.to_string_lossy()
            .eq_ignore_ascii_case(&root.to_string_lossy())
    });
    let fs_name = from_wide(&fs_name);
    let label = from_wide(&label);
    Ok(Volume {
        root: root.to_path_buf(),
        label: (!label.is_empty()).then_some(label),
        fs: FsKind::from_os_name(&fs_name),
        fs_name,
        total_bytes: total,
        free_bytes: available,
        cluster_size,
        sector_size,
        serial: (serial != 0).then(|| format!("{serial:08X}")),
        kind,
        bus,
        model,
        system,
        read_only: flags & FILE_READ_ONLY_VOLUME != 0,
        partition,
    })
}

pub fn list_volumes() -> Vec<Volume> {
    let mut buf = vec![0u16; 1024];
    // SAFETY: the buffer is valid for its length.
    let len = unsafe { GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr()) } as usize;
    buf.truncate(len.min(buf.len()));
    buf.split(|&c| c == 0)
        .filter(|root| !root.is_empty())
        .map(|root| PathBuf::from(OsString::from_wide(root)))
        // Empty card readers and optical drives fail here; skip them.
        .filter_map(|root| query(&root).ok())
        .collect()
}

pub fn volume_for_path(path: &Path) -> io::Result<Volume> {
    let absolute = plain(&std::path::absolute(path)?);
    let path_w = wide(absolute.as_os_str());
    let mut root = vec![0u16; PATH_BUF];
    // SAFETY: `path_w` is NUL-terminated; `root` is valid for its length.
    let ok = unsafe { GetVolumePathNameW(path_w.as_ptr(), root.as_mut_ptr(), root.len() as u32) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    query(&plain(Path::new(&from_wide(&root))))
}

/// How many runs of contiguous clusters `file` occupies: 1 for a file OPL
/// can mount as a VMC, 0 for an empty one. Asks the file system for the
/// file's extents (`FSCTL_GET_RETRIEVAL_POINTERS`, which FAT32, exFAT and
/// NTFS answer) and merges extents that continue each other on disk.
pub fn fragments(file: &File) -> io::Result<u32> {
    const ERROR_HANDLE_EOF: i32 = 38;
    const ERROR_MORE_DATA: i32 = 234;
    // Header (ExtentCount, padding, StartingVcn), then 16-byte extents.
    const HEADER: usize = 16;
    let mut input = STARTING_VCN_INPUT_BUFFER { StartingVcn: 0 };
    // u64 keeps the buffer aligned the way the file system expects.
    let mut out = vec![0u64; 1024];
    let mut runs = Runs::default();
    loop {
        let mut returned = 0u32;
        // SAFETY: the handle belongs to `file`, which outlives the call;
        // `input` and `out` are valid for the sizes passed, and `returned`
        // is a valid out pointer. No OVERLAPPED: the call is synchronous.
        let ok = unsafe {
            DeviceIoControl(
                file.as_raw_handle(),
                FSCTL_GET_RETRIEVAL_POINTERS,
                (&raw const input).cast(),
                size_of::<STARTING_VCN_INPUT_BUFFER>() as u32,
                out.as_mut_ptr().cast(),
                (out.len() * size_of::<u64>()) as u32,
                &mut returned,
                ptr::null_mut(),
            )
        };
        let more = if ok == 0 {
            let err = io::Error::last_os_error();
            match err.raw_os_error() {
                // No clusters at all: an empty file.
                Some(ERROR_HANDLE_EOF) => return Ok(runs.count()),
                Some(ERROR_MORE_DATA) => true,
                _ => return Err(err),
            }
        } else {
            false
        };
        let bytes: Vec<u8> = out.iter().flat_map(|w| w.to_ne_bytes()).collect();
        let at = |offset: usize| -> i64 {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&bytes[offset..offset + 8]);
            i64::from_ne_bytes(raw)
        };
        let count = u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let count = count.min((bytes.len() - HEADER) / 16);
        let extents: Vec<(i64, i64)> = (0..count)
            .map(|index| (at(HEADER + index * 16), at(HEADER + index * 16 + 8)))
            .collect();
        // Extents are (next VCN, LCN): each one ends where the next begins.
        let mut vcn = at(8);
        for &(next_vcn, lcn) in &extents {
            // LCN -1: a hole or a compressed run.
            let start = u64::try_from(lcn).ok();
            runs.add(start, u64::try_from(next_vcn - vcn).unwrap_or(0));
            vcn = next_vcn;
        }
        if !more || count == 0 {
            return Ok(runs.count());
        }
        input.StartingVcn = vcn;
    }
}

/// Reserves `len` bytes for `file` without changing its size, so a
/// sequential write lands in as few fragments as the volume allows.
pub fn preallocate(file: &File, len: u64) -> io::Result<()> {
    let info = FILE_ALLOCATION_INFO {
        AllocationSize: i64::try_from(len)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "size too large"))?,
    };
    // SAFETY: the handle belongs to `file`, which outlives the call; `info`
    // is a valid FILE_ALLOCATION_INFO of the size passed.
    let ok = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileAllocationInfo,
            (&raw const info).cast(),
            size_of::<FILE_ALLOCATION_INFO>() as u32,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Makes renames and removals in `dir` durable: flushes the folder, then
/// the root of its volume. NTFS flushes a folder's own metadata; FAT
/// flushes nothing for a folder but the root, which flushes the whole
/// volume (fastfat `flush.c`, Windows-driver-samples at bb68ed5e77). On a
/// hot-pluggable stick fastfat writes metadata through anyway, unless
/// "Better performance" turned write caching on (`strucsup.c`,
/// `VCB_STATE_FLAG_DEFERRED_FLUSH`): then only this flush puts a rename
/// on the stick before the journal says it happened.
pub fn sync_dir(dir: &Path) -> io::Result<()> {
    flush_folder(dir)?;
    match dir.ancestors().last() {
        Some(root) if root != dir => flush_folder(root),
        _ => Ok(()),
    }
}

fn flush_folder(path: &Path) -> io::Result<()> {
    /// ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED.
    const UNSUPPORTED: [i32; 2] = [1, 50];
    let flushed = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .and_then(|folder| folder.sync_all());
    match flushed {
        // Not every folder opens for writing (the root of a system disk
        // for a standard user) or flushes (some network redirectors): the
        // rename stands as it would without a flush.
        Err(err)
            if err.kind() == io::ErrorKind::PermissionDenied
                || err
                    .raw_os_error()
                    .is_some_and(|code| UNSUPPORTED.contains(&code)) =>
        {
            Ok(())
        }
        other => other,
    }
}
