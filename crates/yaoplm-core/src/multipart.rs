//! A `ul` game's chunk files read as one image.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::PathBuf;

/// Concatenates part files. OPL addresses part `n` at `n * 1 GiB`, so every
/// part but the last must be exactly that long; this reader uses the real
/// lengths and leaves that check to the caller.
#[derive(Debug)]
pub struct MultiPartFile {
    parts: Vec<(PathBuf, u64)>,
    len: u64,
    pos: u64,
    open: Option<(usize, File)>,
}

impl MultiPartFile {
    pub fn new(parts: Vec<(PathBuf, u64)>) -> Self {
        let len = parts.iter().map(|(_, len)| len).sum();
        Self {
            parts,
            len,
            pos: 0,
            open: None,
        }
    }
}

impl Read for MultiPartFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut start = 0;
        for (index, (path, len)) in self.parts.iter().enumerate() {
            if self.pos < start + len {
                let offset = self.pos - start;
                if self.open.as_ref().is_none_or(|(open, _)| *open != index) {
                    self.open = Some((index, File::open(path)?));
                }
                let Some((_, file)) = self.open.as_mut() else {
                    return Ok(0);
                };
                file.seek(SeekFrom::Start(offset))?;
                let want = usize::try_from(len - offset)
                    .unwrap_or(usize::MAX)
                    .min(buf.len());
                let n = file.read(&mut buf[..want])?;
                self.pos += n as u64;
                return Ok(n);
            }
            start += len;
        }
        Ok(0)
    }
}

impl Seek for MultiPartFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(d) => self.len.checked_add_signed(d),
            SeekFrom::Current(d) => self.pos.checked_add_signed(d),
        };
        self.pos = target
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        Ok(self.pos)
    }
}
