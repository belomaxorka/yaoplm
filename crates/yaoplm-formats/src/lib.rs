//! Readers and writers for the on-disk formats OPL uses: ISO9660 and
//! `SYSTEM.CNF`, `ul.cfg` and `USBA_crc32`, per-game CFG, ZSO, CUE/BIN, ART
//! naming, VMC, CHT and APPS.
//!
//! One format per module. Each module links the section of
//! `docs/research/02-opl-formats.md` and the OPL source file it mirrors.
//! Synchronous by design: no tokio, tauri or axum (ADR-0003).

pub mod iso_name;
pub mod media;
pub mod system_cnf;
pub mod ul_cfg;
pub mod usba_crc32;

pub use media::Media;
