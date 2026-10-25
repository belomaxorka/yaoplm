//! Per-OS volume queries, file preallocation and fragment counts.

#[cfg(target_os = "linux")]
mod fiemap;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod log2phys;
#[cfg(target_os = "macos")]
mod macos;
#[cfg_attr(
    not(any(windows, target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
mod runs;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use fiemap::fragments;
#[cfg(target_os = "linux")]
pub use linux::{list_volumes, volume_for_path};
#[cfg(target_os = "macos")]
pub use log2phys::fragments;
#[cfg(target_os = "macos")]
pub use macos::{list_volumes, volume_for_path};
#[cfg(unix)]
pub use unix::preallocate;
#[cfg(windows)]
pub use windows::{fragments, list_volumes, preallocate, sync_dir, volume_for_path};

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn fragments(_file: &std::fs::File) -> std::io::Result<u32> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fragment counts are not implemented on this OS",
    ))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub use fallback::{list_volumes, volume_for_path};

#[cfg(not(any(windows, unix)))]
pub use fallback::preallocate;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod fallback {
    use std::io;
    use std::path::Path;

    use crate::volume::Volume;

    pub fn list_volumes() -> Vec<Volume> {
        Vec::new()
    }

    pub fn volume_for_path(_path: &Path) -> io::Result<Volume> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "volume queries are not implemented on this OS",
        ))
    }

    #[cfg(not(unix))]
    pub fn preallocate(_file: &std::fs::File, _len: u64) -> io::Result<()> {
        Ok(())
    }
}
