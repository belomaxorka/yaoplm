//! Per-OS volume queries and file preallocation.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::{list_volumes, volume_for_path};
#[cfg(target_os = "macos")]
pub use macos::{list_volumes, volume_for_path};
#[cfg(unix)]
pub use unix::preallocate;
#[cfg(windows)]
pub use windows::{list_volumes, preallocate, volume_for_path};

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
