//! CRC-32, SHA-1 and MD5 in one pass over a stream (GAME-5).
//!
//! The caller's thread reads 4 MiB blocks and updates CRC-32 itself (it runs
//! at memory speed); SHA-1 and MD5 each get every block on a thread of their
//! own. Blocks are shared, not copied, and at most a few are in flight, so
//! memory stays bounded whatever the image size. Redump publishes all three
//! hashes; SHA-1 decides a match, CRC-32 and MD5 are shown for people who
//! compare by hand (HDL Batch Installer, for example, prints MD5 only).

use std::io::{self, Read};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;

use md5::Md5;
use sha1::{Digest, Sha1};

/// Read size: large enough for full disk bandwidth, small enough to cancel
/// promptly.
pub const BLOCK_SIZE: usize = 4 << 20;
/// Blocks queued per hasher thread.
const IN_FLIGHT: usize = 3;

/// Which hashes to compute besides CRC-32 and SHA-1, which are always on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Algorithms {
    pub md5: bool,
}

/// Hashes of one stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hashes {
    /// Bytes hashed.
    pub size: u64,
    pub crc32: u32,
    pub sha1: [u8; 20],
    pub md5: Option<[u8; 16]>,
}

impl Hashes {
    /// Lower-case hex, as Redump writes it.
    pub fn crc32_hex(&self) -> String {
        format!("{:08x}", self.crc32)
    }

    pub fn sha1_hex(&self) -> String {
        hex(&self.sha1)
    }

    pub fn md5_hex(&self) -> Option<String> {
        self.md5.as_ref().map(|m| hex(m))
    }
}

/// Lower-case hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `N` bytes from exactly `2 * N` hex digits, either case.
pub fn parse_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    let text = text.trim();
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[derive(Debug, thiserror::Error)]
pub enum HashError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cancelled")]
    Cancelled,
}

fn read_full(reader: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(filled)
}

type Block = Arc<Vec<u8>>;

/// A hasher on its own thread, fed through a bounded queue.
fn spawn_hasher<'scope, D: Digest + Send + 'scope>(
    scope: &'scope thread::Scope<'scope, '_>,
) -> (SyncSender<Block>, thread::ScopedJoinHandle<'scope, D>) {
    let (tx, rx): (SyncSender<Block>, Receiver<Block>) = sync_channel(IN_FLIGHT);
    let handle = scope.spawn(move || {
        let mut digest = D::new();
        for block in rx {
            digest.update(&block[..]);
        }
        digest
    });
    (tx, handle)
}

fn joined<D>(handle: thread::ScopedJoinHandle<'_, D>) -> io::Result<D> {
    handle
        .join()
        .map_err(|_| io::Error::other("a hashing thread panicked"))
}

/// Hashes everything `reader` yields. `stop` is polled between blocks and
/// cancels the run; `progress` gets the bytes hashed so far after every
/// block.
pub fn hash_reader(
    reader: &mut dyn Read,
    algorithms: Algorithms,
    stop: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> Result<Hashes, HashError> {
    thread::scope(|scope| {
        let (sha_tx, sha) = spawn_hasher::<Sha1>(scope);
        let md5 = algorithms.md5.then(|| spawn_hasher::<Md5>(scope));
        let mut crc = crc32fast::Hasher::new();
        let mut size = 0u64;
        let outcome = loop {
            if stop() {
                break Err(HashError::Cancelled);
            }
            let mut block = vec![0u8; BLOCK_SIZE];
            let n = match read_full(reader, &mut block) {
                Ok(n) => n,
                Err(err) => break Err(HashError::Io(err)),
            };
            if n == 0 {
                break Ok(());
            }
            block.truncate(n);
            crc.update(&block);
            size += n as u64;
            let block = Arc::new(block);
            // A send only fails when the thread is gone; join reports why.
            let _ = sha_tx.send(Arc::clone(&block));
            if let Some((tx, _)) = &md5 {
                let _ = tx.send(block);
            }
            progress(size);
        };
        // Closing the queues ends the hasher threads.
        drop(sha_tx);
        let md5 = md5.map(|(tx, handle)| {
            drop(tx);
            handle
        });
        let sha1 = joined(sha)?;
        let md5 = md5.map(joined).transpose()?;
        outcome?;
        Ok(Hashes {
            size,
            crc32: crc.finalize(),
            sha1: sha1.finalize().into(),
            md5: md5.map(|d| d.finalize().into()),
        })
    })
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use super::*;

    #[test]
    fn known_vectors() {
        let hashes = hash_reader(
            &mut Cursor::new(b"The quick brown fox jumps over the lazy dog"),
            Algorithms { md5: true },
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(hashes.size, 43);
        assert_eq!(hashes.crc32_hex(), "414fa339");
        assert_eq!(
            hashes.sha1_hex(),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
        assert_eq!(
            hashes.md5_hex().as_deref(),
            Some("9e107d9d372bb6826bd81d3542a419d6")
        );

        let empty = hash_reader(
            &mut io::empty(),
            Algorithms::default(),
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(empty.size, 0);
        assert_eq!(empty.crc32, 0);
        assert_eq!(empty.sha1_hex(), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(empty.md5, None);
    }

    #[test]
    fn blocks_match_one_shot_hashes() {
        // Several blocks plus a partial one, through the threaded path.
        let data: Vec<u8> = (0..BLOCK_SIZE * 2 + 12_345)
            .map(|i| (i * 31 % 251) as u8)
            .collect();
        let mut seen = Vec::new();
        let hashes = hash_reader(
            &mut Cursor::new(&data),
            Algorithms { md5: true },
            &|| false,
            &mut |n| seen.push(n),
        )
        .unwrap();
        assert_eq!(hashes.size, data.len() as u64);
        assert_eq!(hashes.crc32, crc32fast::hash(&data));
        let sha1: [u8; 20] = Sha1::digest(&data).into();
        assert_eq!(hashes.sha1, sha1);
        let md5: [u8; 16] = Md5::digest(&data).into();
        assert_eq!(hashes.md5, Some(md5));
        assert_eq!(seen.len(), 3);
        assert_eq!(seen.last(), Some(&(data.len() as u64)));
    }

    #[test]
    fn cancel_and_errors() {
        let calls = Cell::new(0);
        let data = vec![0u8; BLOCK_SIZE * 3];
        let err = hash_reader(
            &mut Cursor::new(&data),
            Algorithms { md5: true },
            &|| {
                calls.set(calls.get() + 1);
                calls.get() > 1
            },
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, HashError::Cancelled));

        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("disk gone"))
            }
        }
        let err =
            hash_reader(&mut Failing, Algorithms::default(), &|| false, &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("disk gone"));
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex::<4>("414FA339"), Some([0x41, 0x4f, 0xa3, 0x39]));
        assert_eq!(parse_hex::<4>(" 414fa339 "), Some([0x41, 0x4f, 0xa3, 0x39]));
        assert_eq!(parse_hex::<4>("414fa33"), None);
        assert_eq!(parse_hex::<4>("414fa33g"), None);
        assert_eq!(parse_hex::<2>("éé"), None);
        assert_eq!(hex(&[0, 0xab]), "00ab");
    }
}
