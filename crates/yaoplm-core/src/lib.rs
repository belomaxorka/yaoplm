//! Domain model (game, library, device, loader profile), library scanner,
//! and the plan → confirm → execute engine for every write.
//!
//! - [`scan_library`] and [`image_info`] read what OPL would see.
//! - [`import::plan_import`] builds a [`plan::Plan`]; nothing is written
//!   while planning.
//! - [`exec::execute`] runs a plan with a journal, rollback before each
//!   commit point and resume after it.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).

pub mod device;
pub mod exec;
mod fsutil;
pub mod import;
mod info;
pub mod journal;
mod model;
mod multipart;
pub mod plan;
mod scan;
pub mod source;

pub use info::{ImageInfo, InfoError, Platform, image_info};
pub use model::{
    App, ArtFile, CfgSummary, CheatSummary, Game, GameFormat, GameLocation, Library, SkipReason,
    Skipped, UlPart, Warning,
};
pub use scan::{CD_MAX_BYTES, OPL_FOLDERS, ScanError, ScanOptions, scan_library};
pub use yaoplm_formats::{LoaderProfile, Media};
