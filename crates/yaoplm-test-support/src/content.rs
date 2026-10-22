use std::io::{self, Read, Seek, SeekFrom};

/// File data that is generated on demand, so multi-gigabyte fixtures cost
/// no disk space or memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Content {
    Bytes(Vec<u8>),
    Zeros(u64),
    /// Deterministic, incompressible bytes derived from the offset and seed.
    Noise {
        len: u64,
        seed: u64,
    },
}

impl Content {
    pub fn len(&self) -> u64 {
        match self {
            Self::Bytes(bytes) => bytes.len() as u64,
            Self::Zeros(len) | Self::Noise { len, .. } => *len,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Fills `buf` with the bytes starting at `offset`; past the end is 0.
    pub fn read_at(&self, offset: u64, buf: &mut [u8]) {
        match self {
            Self::Bytes(bytes) => {
                for (i, b) in buf.iter_mut().enumerate() {
                    let at = offset + i as u64;
                    *b = usize::try_from(at)
                        .ok()
                        .and_then(|at| bytes.get(at))
                        .copied()
                        .unwrap_or(0);
                }
            }
            Self::Zeros(_) => buf.fill(0),
            Self::Noise { len, seed } => {
                let mut word = (u64::MAX, [0u8; 8]);
                for (i, b) in buf.iter_mut().enumerate() {
                    let at = offset + i as u64;
                    if at >= *len {
                        *b = 0;
                        continue;
                    }
                    if word.0 != at / 8 {
                        word = (at / 8, noise_word(*seed, at / 8));
                    }
                    *b = word.1[(at % 8) as usize];
                }
            }
        }
    }
}

fn noise_word(seed: u64, word: u64) -> [u8; 8] {
    // splitmix64 of the 8-byte word, so neighbouring bytes look unrelated.
    let mut z = seed ^ word.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    z.to_le_bytes()
}

/// A large file that exists only as a [`Content`] (TESTING.md
/// `VirtualLargeFile`).
#[derive(Debug, Clone)]
pub struct VirtualFile {
    content: Content,
    pos: u64,
}

impl VirtualFile {
    pub fn new(content: Content) -> Self {
        Self { content, pos: 0 }
    }

    pub fn len(&self) -> u64 {
        self.content.len()
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }
}

impl Read for VirtualFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.len().saturating_sub(self.pos);
        let n = usize::try_from(left).unwrap_or(usize::MAX).min(buf.len());
        self.content.read_at(self.pos, &mut buf[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for VirtualFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = resolve_seek(self.pos, self.len(), pos)?;
        Ok(self.pos)
    }
}

pub(crate) fn resolve_seek(current: u64, len: u64, pos: SeekFrom) -> io::Result<u64> {
    let target = match pos {
        SeekFrom::Start(n) => Some(n),
        SeekFrom::End(delta) => len.checked_add_signed(delta),
        SeekFrom::Current(delta) => current.checked_add_signed(delta),
    };
    target.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_file_reads_and_seeks() {
        let mut file = VirtualFile::new(Content::Noise {
            len: 5 << 30,
            seed: 1,
        });
        assert_eq!(file.len(), 5 << 30);
        let mut a = [0u8; 16];
        file.seek(SeekFrom::Start(4 << 30)).unwrap();
        file.read_exact(&mut a).unwrap();
        let mut b = [0u8; 16];
        Content::Noise {
            len: 5 << 30,
            seed: 1,
        }
        .read_at(4 << 30, &mut b);
        assert_eq!(a, b);
        assert_ne!(a, [0u8; 16]);

        file.seek(SeekFrom::End(-4)).unwrap();
        let mut tail = Vec::new();
        file.read_to_end(&mut tail).unwrap();
        assert_eq!(tail.len(), 4);
    }

    #[test]
    fn bytes_pad_with_zeros() {
        let mut buf = [9u8; 4];
        Content::Bytes(b"ab".to_vec()).read_at(1, &mut buf);
        assert_eq!(buf, [b'b', 0, 0, 0]);
    }
}
