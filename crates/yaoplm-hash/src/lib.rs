//! Dump verification (GAME-5, META-2): one read of an image yields CRC-32,
//! SHA-1 and, on request, MD5; the result is matched against a Redump DAT.
//!
//! - [`digest`]: the single-pass pipeline. Each hash runs on its own thread,
//!   so the slowest one, not their sum, bounds the speed (SPEC §6).
//! - [`dat`]: Logiqx XML datafiles as Redump publishes them
//!   (`redump.info/datfile/ps2/serial,version`).
//! - [`redump`]: an index over a datafile and the verdict for one image:
//!   verified, bad dump or unknown.
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003). Downloading
//! the DAT is `yaoplm-net`'s job; this crate only reads it.

pub mod dat;
pub mod digest;
pub mod redump;

pub use digest::{Algorithms, HashError, Hashes, hash_reader};
pub use redump::{RedumpIndex, Verdict};
