//! Volumes, file systems and safe sequential writes.
//!
//! - [`volume`]: which volumes are mounted, their file system, free space,
//!   cluster size and serial number (DEV-1).
//! - [`advice`]: a read-only verdict on a volume for a loader profile
//!   (DEV-2, research/06 §6.1).
//! - [`fs`]: [`DeviceFs`], the only way the rest of YAOPLM writes to a
//!   device. Paths are [`RelPath`]s, so nothing can be written outside the
//!   selected root (CLAUDE.md rule 1).
//! - [`copy`]: sequential copies with preallocation, progress and
//!   cancellation (IMP-7).
//! - [`source`]: read access to import sources on the host, which are never
//!   written ([`SourceFs`]).
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003). OS calls are
//! made directly (ADR-0019); `unsafe` is confined to `sys/windows.rs`,
//! `sys/fiemap.rs` and `sys/log2phys.rs` (ADR-0037).

pub mod advice;
pub mod copy;
pub mod fs;
pub mod source;
mod sys;
pub mod volume;

pub use advice::{Advice, DiagnoseOptions, Level, diagnose};
pub use copy::{CancelToken, CopyError, copy_exact};
pub use fs::{DeviceFile, DeviceFs, DirEntry, FileMeta, FsInfo, HostFs, PathError, RelPath};
pub use source::{HostSources, ReadSeek, SourceFs, SourceMeta};
pub use volume::{
    Bus, FsKind, PartitionInfo, PartitionStyle, Volume, VolumeKind, list_volumes, volume_for_path,
};
