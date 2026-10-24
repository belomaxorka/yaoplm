//! Readers and writers for the on-disk formats OPL uses: ISO9660 and
//! `SYSTEM.CNF`, `ul.cfg` and `USBA_crc32`, per-game CFG, ZSO, CUE/BIN, ART
//! naming, game settings, VMC, CHT and APPS; and the containers images arrive in: CHD,
//! zip and 7z (IMP-1b).
//!
//! One format per module. Each module links the section of
//! `docs/research/02-opl-formats.md` and the OPL source file it mirrors.
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).

pub mod apps;
pub mod archive;
pub mod art;
pub mod cfg;
#[cfg(feature = "chd")]
pub mod chd;
pub mod cht;
pub mod cue;
pub mod game_cfg;
pub mod games_bin;
pub mod image;
pub mod iso9660;
pub mod iso_name;
pub mod media;
pub mod profile;
pub mod serial;
pub mod system_cnf;
pub mod tar;
pub mod ul_cfg;
pub mod usba_crc32;
pub mod vmc;
pub mod zso;

pub use media::Media;
pub use profile::LoaderProfile;
