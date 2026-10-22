//! Sequential copies with progress and cancellation (IMP-7, IMP-9).
//!
//! One thread reads ahead while the caller's thread writes, so a slow
//! source (a USB hard drive) and a slow target (a USB stick) overlap. Data
//! is written strictly in order, in large blocks: with the space reserved
//! up front this keeps each file in as few fragments as the volume allows
//! (OPL loads at most 64 per game, research/02 §4.7).

use std::io::{self, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;

/// 4 MiB: large enough for full USB bandwidth, small enough for responsive
/// cancellation and progress.
pub const BLOCK_SIZE: usize = 4 << 20;

/// Shared flag a job checks between blocks.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CopyError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cancelled")]
    Cancelled,
    #[error("the source ended after {got} of {expected} bytes")]
    ShortSource { expected: u64, got: u64 },
}

fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
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

/// Copies exactly `len` bytes from `reader` to `out`. `progress` gets the
/// bytes written so far after every block. Reads stop at `len`, so the same
/// reader can feed several calls (one per `ul` chunk).
pub fn copy_exact<R: Read + Send>(
    reader: &mut R,
    out: &mut dyn Write,
    len: u64,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(u64),
) -> Result<(), CopyError> {
    if len == 0 {
        return Ok(());
    }
    // Two blocks in flight: one being read, one being written.
    let (full_tx, full_rx) = mpsc::sync_channel::<io::Result<Vec<u8>>>(1);
    let (empty_tx, empty_rx) = mpsc::sync_channel::<Vec<u8>>(2);
    for _ in 0..2 {
        let _ = empty_tx.send(Vec::with_capacity(BLOCK_SIZE));
    }
    thread::scope(|scope| {
        // Owned by this closure, so they drop before the scope joins the
        // reader: an early return (error, cancel) unblocks it.
        let full_rx = full_rx;
        let empty_tx = empty_tx;
        scope.spawn(move || {
            let mut left = len;
            while left > 0 {
                let Ok(mut buf) = empty_rx.recv() else {
                    return;
                };
                let want = usize::try_from(left).unwrap_or(BLOCK_SIZE).min(BLOCK_SIZE);
                buf.resize(want, 0);
                let result = read_full(reader, &mut buf).map(|n| {
                    buf.truncate(n);
                    buf
                });
                let stop = !matches!(&result, Ok(b) if !b.is_empty());
                if let Ok(b) = &result {
                    left -= b.len() as u64;
                }
                if full_tx.send(result).is_err() || stop {
                    return;
                }
            }
        });

        let mut done = 0u64;
        while done < len {
            if cancel.is_cancelled() {
                return Err(CopyError::Cancelled);
            }
            let block = match full_rx.recv() {
                Ok(block) => block?,
                Err(_) => Vec::new(),
            };
            if block.is_empty() {
                return Err(CopyError::ShortSource {
                    expected: len,
                    got: done,
                });
            }
            out.write_all(&block)?;
            done += block.len() as u64;
            progress(done);
            // The reader may already be gone after its last block.
            let _ = empty_tx.send(block);
        }
        out.flush()?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counting(u64);

    impl Read for Counting {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            for b in buf.iter_mut() {
                *b = (self.0 % 251) as u8;
                self.0 += 1;
            }
            Ok(buf.len())
        }
    }

    #[test]
    fn copies_exactly_len_and_reports_progress() {
        let mut reader = Counting(0);
        let mut out = Vec::new();
        let mut seen = Vec::new();
        let len = (BLOCK_SIZE * 2 + 100) as u64;
        copy_exact(&mut reader, &mut out, len, &CancelToken::new(), &mut |n| {
            seen.push(n)
        })
        .unwrap();
        assert_eq!(out.len() as u64, len);
        assert!(out.iter().enumerate().all(|(i, &b)| b == (i % 251) as u8));
        assert_eq!(seen.last(), Some(&len));
        // The next call continues where this one stopped.
        let mut next = Vec::new();
        copy_exact(&mut reader, &mut next, 10, &CancelToken::new(), &mut |_| {}).unwrap();
        assert_eq!(next[0], (len % 251) as u8);
    }

    #[test]
    fn short_source_and_cancel() {
        let mut short: &[u8] = &[1, 2, 3];
        let err = copy_exact(
            &mut short,
            &mut Vec::new(),
            10,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(
            err,
            CopyError::ShortSource {
                expected: 10,
                got: 3
            }
        ));

        let cancel = CancelToken::new();
        cancel.cancel();
        let err =
            copy_exact(&mut Counting(0), &mut Vec::new(), 10, &cancel, &mut |_| {}).unwrap_err();
        assert!(matches!(err, CopyError::Cancelled));
    }

    #[test]
    fn read_errors_propagate() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("disk gone"))
            }
        }
        let err = copy_exact(
            &mut Failing,
            &mut Vec::new(),
            10,
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.to_string().contains("disk gone"));
    }
}
