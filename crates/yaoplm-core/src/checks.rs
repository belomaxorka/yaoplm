//! Dump checks while games are added (GAME-5): the copy to the device
//! hashes what it writes, so the device is never read a second time.
//!
//! - The image is hashed as OPL reads it: the ISO sectors, or the ISO
//!   inside a ZSO copied as it is ([`StreamDecoder`]).
//! - A raw CD dump (a BIN or CHD track of 2352-byte sectors) also has its
//!   data track hashed, sector by sector as it is decoded: Redump hashes
//!   CDs that way, so only this hash can match its DAT (ADR-0030).
//!
//! Hashes come out only when a source went through whole, from its first
//! byte, in one pass; a resumed or failed write yields none, and the game
//! is checked later by reading it. So does a write whose hashing threads
//! could not be started: the check is lost, not the write. The copy needs
//! a thread of its own as well; without it the write fails like any I/O
//! error ([`yaoplm_device::copy_exact`]).

use std::io::{self, Read};
use std::sync::{Arc, Mutex, PoisonError};

use yaoplm_formats::cue::RAW_SECTOR_SIZE;
use yaoplm_formats::iso9660::SECTOR_SIZE;
use yaoplm_formats::zso::StreamDecoder;
use yaoplm_hash::{Algorithms, Hashes, StreamHasher};

use crate::plan::{Check, Source, SourceKind};

/// Hashes taken while a plan wrote an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checked {
    /// The ISO as OPL reads it.
    pub image: Hashes,
    /// The raw data track of a CD dump, as Redump hashes it.
    pub track: Option<Hashes>,
}

/// A hasher fed from inside a stream.
#[derive(Debug)]
pub(crate) struct Tap {
    hasher: Option<StreamHasher>,
    /// For a ZSO copied as it is: the ISO comes out of the copied bytes.
    zso: Option<StreamDecoder>,
}

pub(crate) type SharedTap = Arc<Mutex<Tap>>;

impl Tap {
    fn shared(zso: bool) -> SharedTap {
        Arc::new(Mutex::new(Self {
            hasher: StreamHasher::new(Algorithms::default()).ok(),
            zso: zso.then(StreamDecoder::new),
        }))
    }

    fn feed(&mut self, bytes: &[u8]) {
        let Some(hasher) = self.hasher.as_mut() else {
            return;
        };
        let lost = match self.zso.as_mut() {
            None => {
                hasher.update(bytes);
                false
            }
            Some(decoder) => decoder
                .push(bytes, &mut |sector| hasher.update(sector))
                .is_err(),
        };
        if lost {
            // A ZSO whose blocks are out of order: it is read later instead.
            self.hasher = None;
        }
    }

    /// The hashes, if exactly `expected` bytes went in (for a ZSO: the
    /// image its header announces, every block decoded).
    fn finish(&mut self, expected: Option<u64>) -> Option<Hashes> {
        let hasher = self.hasher.take()?;
        let expected = match (&self.zso, expected) {
            (Some(decoder), _) if decoder.is_done() => decoder.sector_count()? * SECTOR_SIZE as u64,
            (Some(_), _) => return None,
            (None, expected) => expected?,
        };
        hasher.finish().ok().filter(|h| h.size == expected)
    }
}

/// Hands `bytes` to the hasher behind `tap`.
pub(crate) fn feed(tap: &SharedTap, bytes: &[u8]) {
    tap.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .feed(bytes);
}

fn finish(tap: &SharedTap, expected: Option<u64>) -> Option<Hashes> {
    tap.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish(expected)
}

/// The hashers of one source being written.
#[derive(Debug)]
pub(crate) struct Taps {
    /// The plan's index of the source.
    pub source: usize,
    image: SharedTap,
    track: Option<SharedTap>,
}

impl Taps {
    pub(crate) fn new(source: usize, check: Check, kind: &SourceKind) -> Self {
        let raw = match kind {
            SourceKind::Chd => true,
            SourceKind::Bin { sector_size, .. } => *sector_size == RAW_SECTOR_SIZE,
            _ => false,
        };
        Self {
            source,
            image: Tap::shared(check == Check::Zso),
            track: (raw && check == Check::Image).then(|| Tap::shared(false)),
        }
    }

    /// The tap for the raw sectors of a CD track, when the source has them.
    pub(crate) fn track(&self) -> Option<SharedTap> {
        self.track.clone()
    }

    /// `inner`, hashed as it is read.
    pub(crate) fn reader<R: Read + Send + 'static>(&self, inner: R) -> TapReader<R> {
        TapReader {
            inner,
            tap: Arc::clone(&self.image),
        }
    }

    /// The hashes of `source` once it went through whole. A CHD of a DVD,
    /// or a CD track stored without its sync and ECC, has no track hashes.
    pub(crate) fn finish(self, source: &Source) -> Option<Checked> {
        let zso = source.check == Some(Check::Zso);
        let image = finish(&self.image, (!zso).then_some(source.len))?;
        let sectors = source.len / SECTOR_SIZE as u64;
        let track = self
            .track
            .as_ref()
            .and_then(|tap| finish(tap, Some(sectors * u64::from(RAW_SECTOR_SIZE))));
        Some(Checked { image, track })
    }
}

/// A reader whose bytes also go to a hasher.
pub(crate) struct TapReader<R> {
    inner: R,
    tap: SharedTap,
}

impl<R: Read> Read for TapReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        feed(&self.tap, &buf[..n]);
        Ok(n)
    }
}
