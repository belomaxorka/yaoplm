//! Domain model (game, library, device, loader profile), library scanner,
//! and the plan → confirm → execute engine for every write.
//!
//! - [`scan_library`] and [`image_info`] read what OPL would see.
//! - [`import::plan_import`], [`ops`] and [`art`] build [`plan::Plan`]s;
//!   nothing is written while planning.
//! - [`verify`] hashes the images on a device for dump verification.
//! - [`exec::execute`] runs a plan with a journal, rollback before each
//!   commit point and resume after it; [`recovery::plan_recovery`] picks up
//!   what a crash left behind.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).

pub mod art;
pub mod device;
pub mod exec;
mod fsutil;
pub mod import;
mod info;
pub mod journal;
mod model;
mod multipart;
pub mod ops;
pub mod plan;
pub mod recovery;
mod scan;
pub mod source;
pub mod titles;
pub mod verify;

pub use info::{ImageInfo, InfoError, Platform, image_info};
pub use model::{
    App, ArtFile, CfgSummary, CheatSummary, Game, GameFormat, GameLocation, Library, SkipReason,
    Skipped, UlPart, Warning,
};
pub use scan::{
    CD_MAX_BYTES, OPL_FOLDERS, ScanError, ScanOptions, scan_library, scan_library_with,
};
pub use yaoplm_formats::{LoaderProfile, Media};
