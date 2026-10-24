//! Linux: where a file lies on the device (IMP-7b).
//!
//! - `FS_IOC_FIEMAP` first: one call returns many extents, no privileges.
//!   The vfat and exfat drivers do not implement it, though (no `.fiemap`
//!   in `fs/fat/file.c` or `fs/exfat/file.c`, torvalds/linux master
//!   2026-10), so on the file systems OPL reads it fails with EOPNOTSUPP.
//! - `FIBMAP` then: both drivers map a block to its device block through
//!   `bmap`, but the kernel answers only callers with `CAP_SYS_RAWIO`
//!   (`ioctl_fibmap` in `fs/ioctl.c`). We ask once per cluster.
//! - Otherwise the count is unknown: `ErrorKind::Unsupported`.
//!
//! One of the modules allowed `unsafe` (ADR-0037): the ioctl structs are
//! owned and sized on the Rust side, every block has a `SAFETY` note.
#![allow(unsafe_code)]

use std::ffi::c_int;
use std::fs::File;
use std::io;

use rustix::io::Errno;
use rustix::ioctl::{Opcode, Updater, ioctl, opcode};

use super::runs::Runs;

/// `struct fiemap` without its trailing extent array (`linux/fiemap.h`).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct FiemapHeader {
    fm_start: u64,
    fm_length: u64,
    fm_flags: u32,
    fm_mapped_extents: u32,
    fm_extent_count: u32,
    fm_reserved: u32,
}

/// `struct fiemap_extent`.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
struct FiemapExtent {
    fe_logical: u64,
    fe_physical: u64,
    fe_length: u64,
    fe_reserved64: [u64; 2],
    fe_flags: u32,
    fe_reserved: [u32; 3],
}

const EXTENTS: usize = 128;

/// The header and room for [`EXTENTS`] extents, in one allocation the
/// kernel fills.
#[repr(C)]
struct Fiemap {
    header: FiemapHeader,
    extents: [FiemapExtent; EXTENTS],
}

/// `_IOWR('f', 11, struct fiemap)`: the size is that of the header only.
const FS_IOC_FIEMAP: Opcode = opcode::read_write::<FiemapHeader>(b'f', 11);
/// `_IO(0x00, 1)` and `_IO(0x00, 2)`, both taking an `int *`.
const FIBMAP: Opcode = opcode::none(0, 1);
const FIGETBSZ: Opcode = opcode::none(0, 2);

const FIEMAP_FLAG_SYNC: u32 = 0x0001;
const FIEMAP_EXTENT_LAST: u32 = 0x0001;
/// Unknown, delayed, encoded, inline or tail data: no usable location.
const FIEMAP_EXTENT_NO_LOCATION: u32 = 0x0002 | 0x0004 | 0x0008 | 0x0200 | 0x0400;

fn unknown(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, why)
}

/// How many runs of contiguous space `file` occupies, or `Unsupported`
/// when the kernel will not tell this process.
pub fn fragments(file: &File) -> io::Result<u32> {
    match fiemap(file) {
        Err(Errno::OPNOTSUPP | Errno::NOTTY) => fibmap(file),
        other => other.map_err(io::Error::from),
    }
}

fn fiemap(file: &File) -> Result<u32, Errno> {
    let mut runs = Runs::default();
    let mut start = 0u64;
    let mut map = Box::new(Fiemap {
        header: FiemapHeader::default(),
        extents: [FiemapExtent::default(); EXTENTS],
    });
    loop {
        map.header = FiemapHeader {
            fm_start: start,
            fm_length: u64::MAX,
            fm_flags: FIEMAP_FLAG_SYNC,
            fm_extent_count: EXTENTS as u32,
            ..FiemapHeader::default()
        };
        // SAFETY: FS_IOC_FIEMAP takes a `struct fiemap` followed by
        // `fm_extent_count` extents; `map` is exactly that, repr(C), and
        // lives until the call returns.
        unsafe { ioctl(file, Updater::<FS_IOC_FIEMAP, Fiemap>::new(&mut *map))? };
        let mapped = (map.header.fm_mapped_extents as usize).min(EXTENTS);
        let extents = &map.extents[..mapped];
        for extent in extents {
            if extent.fe_flags & FIEMAP_EXTENT_NO_LOCATION != 0 {
                return Err(Errno::OPNOTSUPP);
            }
            runs.add(Some(extent.fe_physical), extent.fe_length);
        }
        let Some(last) = extents.last() else {
            return Ok(runs.count());
        };
        let next = last.fe_logical.saturating_add(last.fe_length);
        if last.fe_flags & FIEMAP_EXTENT_LAST != 0 || next <= start {
            return Ok(runs.count());
        }
        start = next;
    }
}

fn fibmap(file: &File) -> io::Result<u32> {
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(0);
    }
    let mut block: c_int = 0;
    // SAFETY: FIGETBSZ writes one `int` through the pointer it is given.
    unsafe { ioctl(file, Updater::<FIGETBSZ, c_int>::new(&mut block)) }.map_err(io::Error::from)?;
    let block = u64::try_from(block)
        .ok()
        .filter(|&b| b > 0)
        .ok_or_else(|| unknown("FIGETBSZ gave no block size"))?;
    // vfat and exfat report the cluster as the statfs block size.
    let cluster = rustix::fs::fstatvfs(file).map_err(io::Error::from)?.f_bsize;
    let step = (cluster / block).max(1);
    let blocks = len.div_ceil(block);
    let mut runs = Runs::default();
    let mut index = 0u64;
    while index < blocks {
        let mut value = c_int::try_from(index).map_err(|_| unknown("file too large for FIBMAP"))?;
        // SAFETY: FIBMAP reads a logical block number from the `int` and
        // writes the device block number back into it.
        match unsafe { ioctl(file, Updater::<FIBMAP, c_int>::new(&mut value)) } {
            Ok(()) => {}
            // No CAP_SYS_RAWIO, no `bmap`, or a block past 2^31.
            Err(Errno::PERM | Errno::INVAL | Errno::RANGE | Errno::NOTTY | Errno::OPNOTSUPP) => {
                return Err(unknown("the kernel maps file blocks for root only"));
            }
            Err(err) => return Err(err.into()),
        }
        // 0: not mapped (a hole).
        let start = u64::try_from(value).ok().filter(|&b| b > 0);
        runs.add(start, step);
        index += step;
    }
    Ok(runs.count())
}
