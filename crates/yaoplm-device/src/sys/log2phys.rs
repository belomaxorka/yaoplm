//! macOS: where a file lies on the device (IMP-7b), through
//! `fcntl(F_LOG2PHYS_EXT)`. Given a file offset and a length, the kernel
//! returns the device offset and how many bytes from there are contiguous
//! (`VNOP_BLOCKMAP`; `sys_fcntl_nocancel` in xnu `bsd/kern/kern_descrip.c`).
//! msdosfs implements the block map; for the user-space FAT and exFAT
//! drivers Apple documents `blockmapFile` as reached through `F_LOG2PHYS`.
//! A file system that answers with an error leaves the count unknown.
//!
//! One of the modules allowed `unsafe` (ADR-0037).
#![allow(unsafe_code)]

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

use super::runs::Runs;

fn unknown() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the file system does not map file offsets to the device",
    )
}

/// How many runs of contiguous space `file` occupies, or `Unsupported`.
pub fn fragments(file: &File) -> io::Result<u32> {
    let len = file.metadata()?.len();
    let mut runs = Runs::default();
    let mut offset = 0u64;
    while offset < len {
        let mut query = libc::log2phys {
            l2p_flags: 0,
            l2p_contigbytes: libc::off_t::try_from(len - offset).map_err(|_| unknown())?,
            l2p_devoffset: libc::off_t::try_from(offset).map_err(|_| unknown())?,
        };
        // SAFETY: F_LOG2PHYS_EXT reads and writes one `struct log2phys`
        // through the pointer; `query` is that struct and outlives the call.
        let result = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_LOG2PHYS_EXT, &raw mut query) };
        if result == -1 {
            let err = io::Error::last_os_error();
            return Err(match err.raw_os_error() {
                Some(
                    libc::ENOTSUP
                    | libc::EOPNOTSUPP
                    | libc::EINVAL
                    | libc::ENOTTY
                    | libc::EPERM
                    | libc::EACCES,
                ) => unknown(),
                _ => err,
            });
        }
        // Copies: the struct is packed.
        let contiguous = query.l2p_contigbytes;
        let device = query.l2p_devoffset;
        let Some(contiguous) = u64::try_from(contiguous).ok().filter(|&n| n > 0) else {
            return Err(unknown());
        };
        // -1: not backed by blocks on the device.
        runs.add(u64::try_from(device).ok(), contiguous);
        offset = offset.saturating_add(contiguous);
    }
    Ok(runs.count())
}
