//! CRC-32, SHA-1 and MD5 in one pass over a stream (GAME-5).
//!
//! CRC-32 runs on the thread that has the bytes (it runs at memory speed);
//! SHA-1 and MD5 each get every 4 MiB block on a thread of their own.
//! Blocks are shared, not copied, and at most a few are in flight, so
//! memory stays bounded whatever the image size. Redump publishes all three
//! hashes; SHA-1 decides a match, CRC-32 and MD5 are shown for people who
//! compare by hand (HDL Batch Installer, for example, prints MD5 only).
//!
//! [`hash_reader`] reads a stream itself; [`StreamHasher`] takes bytes that
//! someone else reads.

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
    // `from_str_radix` alone would take a sign: `+1+2+3+4`.
    if text.len() != N * 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
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

/// A digest on its own thread, fed through a bounded queue.
struct Worker<D> {
    queue: Option<SyncSender<Block>>,
    handle: Option<thread::JoinHandle<D>>,
}

impl<D: Digest + Send + 'static> Worker<D> {
    fn spawn() -> Self {
        let (tx, rx): (SyncSender<Block>, Receiver<Block>) = sync_channel(IN_FLIGHT);
        let handle = thread::spawn(move || {
            let mut digest = D::new();
            for block in rx {
                digest.update(&block[..]);
            }
            digest
        });
        Self {
            queue: Some(tx),
            handle: Some(handle),
        }
    }

    fn send(&self, block: Block) {
        // A send only fails when the thread is gone; `finish` says why.
        if let Some(queue) = &self.queue {
            let _ = queue.send(block);
        }
    }

    fn finish(mut self) -> io::Result<D> {
        // Closing the queue ends the thread.
        self.queue = None;
        self.handle
            .take()
            .ok_or_else(|| io::Error::other("a hashing thread is gone"))?
            .join()
            .map_err(|_| io::Error::other("a hashing thread panicked"))
    }
}

impl<D> Drop for Worker<D> {
    fn drop(&mut self) {
        // Abandoned: closing the queue lets the thread end on its own.
        self.queue = None;
    }
}

/// Hashes bytes handed in pieces of any size: the copy of an image to a
/// device feeds what it reads, so the device is not read a second time.
/// Pieces are gathered into blocks for the SHA-1 and MD5 threads; dropping
/// the hasher without [`StreamHasher::finish`] stops them.
pub struct StreamHasher {
    crc: crc32fast::Hasher,
    size: u64,
    pending: Vec<u8>,
    sha1: Worker<Sha1>,
    md5: Option<Worker<Md5>>,
}

impl std::fmt::Debug for StreamHasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamHasher")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl StreamHasher {
    pub fn new(algorithms: Algorithms) -> Self {
        Self {
            crc: crc32fast::Hasher::new(),
            size: 0,
            pending: Vec::new(),
            sha1: Worker::spawn(),
            md5: algorithms.md5.then(Worker::spawn),
        }
    }

    /// Bytes hashed so far.
    pub fn size(&self) -> u64 {
        self.size
    }

    fn send(&mut self, block: Vec<u8>) {
        let block = Arc::new(block);
        if let Some(md5) = &self.md5 {
            md5.send(Arc::clone(&block));
        }
        self.sha1.send(block);
    }

    pub fn update(&mut self, bytes: &[u8]) {
        self.crc.update(bytes);
        self.size += bytes.len() as u64;
        let mut rest = bytes;
        while !rest.is_empty() {
            if self.pending.capacity() == 0 {
                self.pending.reserve_exact(BLOCK_SIZE);
            }
            let n = rest.len().min(BLOCK_SIZE - self.pending.len());
            self.pending.extend_from_slice(&rest[..n]);
            rest = &rest[n..];
            if self.pending.len() == BLOCK_SIZE {
                let block = std::mem::take(&mut self.pending);
                self.send(block);
            }
        }
    }

    /// A block the caller owns, passed on without a copy.
    fn update_block(&mut self, block: Vec<u8>) {
        if !self.pending.is_empty() {
            self.update(&block);
            return;
        }
        self.crc.update(&block);
        self.size += block.len() as u64;
        self.send(block);
    }

    pub fn finish(mut self) -> io::Result<Hashes> {
        let rest = std::mem::take(&mut self.pending);
        if !rest.is_empty() {
            self.send(rest);
        }
        let Self {
            crc,
            size,
            sha1,
            md5,
            ..
        } = self;
        let sha1 = sha1.finish()?;
        let md5 = md5.map(Worker::finish).transpose()?;
        Ok(Hashes {
            size,
            crc32: crc.finalize(),
            sha1: sha1.finalize().into(),
            md5: md5.map(|d| d.finalize().into()),
        })
    }
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
    let mut hasher = StreamHasher::new(algorithms);
    loop {
        if stop() {
            return Err(HashError::Cancelled);
        }
        let mut block = vec![0u8; BLOCK_SIZE];
        let n = read_full(reader, &mut block)?;
        if n == 0 {
            break;
        }
        block.truncate(n);
        hasher.update_block(block);
        progress(hasher.size());
    }
    Ok(hasher.finish()?)
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

    /// At most 1000 bytes per read; fails once `fail_at` bytes are out.
    struct Trickle<'a> {
        data: &'a [u8],
        pos: usize,
        fail_at: Option<usize>,
    }

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.fail_at.is_some_and(|at| self.pos >= at) {
                return Err(io::Error::other("cable pulled"));
            }
            let n = buf.len().min(1000).min(self.data.len() - self.pos);
            buf[..n].copy_from_slice(&self.data[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn short_reads_and_late_errors() {
        let data: Vec<u8> = (0..BLOCK_SIZE + 4321).map(|i| (i % 253) as u8).collect();
        let mut reader = Trickle {
            data: &data,
            pos: 0,
            fail_at: None,
        };
        let hashes = hash_reader(
            &mut reader,
            Algorithms { md5: true },
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(hashes.size, data.len() as u64);
        assert_eq!(hashes.crc32, crc32fast::hash(&data));
        let sha1: [u8; 20] = Sha1::digest(&data).into();
        assert_eq!(hashes.sha1, sha1);

        // The error comes after a whole block went to the hashing threads.
        let mut reader = Trickle {
            data: &data,
            pos: 0,
            fail_at: Some(BLOCK_SIZE + 2000),
        };
        let err = hash_reader(
            &mut reader,
            Algorithms { md5: true },
            &|| false,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("cable pulled"), "{err}");
    }

    #[test]
    fn pieces_hash_like_one_stream() {
        let data: Vec<u8> = (0..BLOCK_SIZE * 2 + 777)
            .map(|i| (i * 7 % 241) as u8)
            .collect();
        let whole = hash_reader(
            &mut Cursor::new(&data),
            Algorithms { md5: true },
            &|| false,
            &mut |_| {},
        )
        .unwrap();
        // Pieces of odd sizes, some crossing block boundaries.
        let mut hasher = StreamHasher::new(Algorithms { md5: true });
        let mut at = 0;
        for size in [1, 2351, 2352, BLOCK_SIZE - 3, 2048, BLOCK_SIZE + 5]
            .iter()
            .cycle()
        {
            if at == data.len() {
                break;
            }
            let end = (at + size).min(data.len());
            hasher.update(&data[at..end]);
            at = end;
        }
        assert_eq!(hasher.size(), data.len() as u64);
        assert_eq!(hasher.finish().unwrap(), whole);

        let empty = StreamHasher::new(Algorithms::default()).finish().unwrap();
        assert_eq!(empty.sha1_hex(), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(empty.md5, None);

        // Abandoned half way: the threads end, nothing hangs.
        let mut hasher = StreamHasher::new(Algorithms { md5: true });
        hasher.update(&data[..BLOCK_SIZE + 1]);
        drop(hasher);
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(parse_hex::<4>("414FA339"), Some([0x41, 0x4f, 0xa3, 0x39]));
        assert_eq!(parse_hex::<4>(" 414fa339 "), Some([0x41, 0x4f, 0xa3, 0x39]));
        assert_eq!(parse_hex::<4>("414fa33"), None);
        assert_eq!(parse_hex::<4>("414fa33g"), None);
        assert_eq!(parse_hex::<4>("+1+2+3+4"), None);
        assert_eq!(parse_hex::<2>("éé"), None);
        assert_eq!(hex(&[0, 0xab]), "00ab");
    }
}
