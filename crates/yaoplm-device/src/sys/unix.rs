use std::fs::File;
use std::io;

/// Reserves `len` bytes without changing the file size (Linux `fallocate`
/// with `FALLOC_FL_KEEP_SIZE`; vfat has it since 4.19). Best effort: the
/// in-kernel exFAT driver refuses, which only costs contiguity.
///
/// macOS is skipped: `F_PREALLOCATE` through rustix also extends the file,
/// and msdosfs zero-fills the extension, doubling the write.
pub fn preallocate(file: &File, len: u64) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        rustix::fs::fallocate(file, rustix::fs::FallocateFlags::KEEP_SIZE, 0, len)
            .map_err(io::Error::from)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, len);
        Ok(())
    }
}
