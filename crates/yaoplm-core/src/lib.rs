//! Domain model (game, library, device, loader profile), library scanner,
//! job primitives and the SQLite cache.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).

mod fsutil;
mod info;
mod model;
mod multipart;
mod scan;

pub use info::{ImageInfo, InfoError, Platform, image_info};
pub use model::{
    App, ArtFile, CfgSummary, CheatSummary, Game, GameFormat, GameLocation, Library, SkipReason,
    Skipped, UlPart, Warning,
};
pub use scan::{CD_MAX_BYTES, OPL_FOLDERS, ScanError, ScanOptions, scan_library};
pub use yaoplm_formats::{LoaderProfile, Media};
