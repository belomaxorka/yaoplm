//! Dump verification (GAME-5, META-2): one read of an image yields CRC-32,
//! SHA-1 and, on request, MD5; the result is matched against a Redump DAT.
//!
//! - [`digest`]: the single-pass pipeline. Each hash runs on its own thread,
//!   so the slowest one, not their sum, bounds the speed (SPEC §6).
//! - [`dat`]: Logiqx XML datafiles as Redump publishes them
//!   (`redump.info/datfile/ps2/serial,version`).
//! - [`redump`]: an index over a datafile and the verdict for one image:
//!   verified, bad dump or unknown.
//! - [`sha256_reader`]: the identity of a loader ELF (LDR-4, LIB-1).
//!
//! Synchronous by design: no tokio, tauri or axum (ADR-0003). Downloading
//! the DAT is `yaoplm-net`'s job; this crate only reads it.

pub mod dat;
pub mod digest;
pub mod redump;

use std::io::{self, Read};

use sha2::{Digest, Sha256};

pub use digest::{Algorithms, HashError, Hashes, StreamHasher, hash_reader};
pub use redump::{RedumpIndex, Verdict};

/// SHA-256 of everything `reader` yields, as lowercase hex: how release
/// assets are identified (`SHA256SUMS`, GitHub `digest`).
pub fn sha256_reader(mut reader: impl Read) -> io::Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod sha256_tests {
    #[test]
    fn known_vectors() {
        assert_eq!(
            super::sha256_reader(&b""[..]).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            super::sha256_reader(&b"abc"[..]).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
