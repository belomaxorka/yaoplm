//! ZSO: LZ4-compressed ISO (OPL 1.2.0 beta and forks only).
//!
//! Format reference: `docs/research/02-opl-formats.md` §12. Mirrors
//! `modules/isofs/zso.h`, `ziso_init()`/`ziso_read_sector()` in
//! `modules/isofs/zso.c` and `read_raw_data()` in `modules/isofs/isofs.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997; the writer follows the layout of
//! `pc/ziso.py` from the same commit. Spec: unknownbrackets/maxcso
//! `README_ZSO.md`.
//!
//! Layout: a 24-byte little-endian header (`ZISO`, header size, total bytes,
//! block size, version, index shift), then `total / 2048 + 1` u32 index
//! entries, then the blocks. Entry bits 0-30 are the block offset shifted
//! right by `align`; bit 31 marks a block stored uncompressed. Compressed
//! blocks are raw LZ4 blocks that decode to exactly 2048 bytes.
//!
//! What OPL assumes, so what we enforce:
//! - The index starts at byte 24 and blocks are 2048 bytes, whatever the
//!   header says.
//! - `align` must be at most 11: `read_raw_data()` divides by `2048 >> align`.
//! - The block count is `total_bytes >> 11`; a partial last block is lost
//!   (ziso.py drops it too).
//! - Compressed blocks are decoded with `LZ4_decompress_fast`, which stops
//!   once 2048 bytes are out, so alignment padding after them is harmless.
//!   Each stored block must fit in 2048 bytes: OPL reads a run of blocks into
//!   the tail of the caller's buffer.

use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::iso9660::{SECTOR_SIZE, Sector, SectorSource};

pub const MAGIC: [u8; 4] = *b"ZISO";
pub const HEADER_SIZE: usize = 24;
pub const BLOCK_SIZE: u32 = 2048;
/// Larger shifts divide by zero in OPL's `read_raw_data()`.
pub const MAX_ALIGN: u8 = 11;
const PLAIN: u32 = 0x8000_0000;
const OFFSET_MASK: u32 = 0x7FFF_FFFF;
/// ziso.py stores a block plain once compression saves less than 5%.
const COMPRESS_THRESHOLD_PERCENT: usize = 95;
/// Index entries read at once, like OPL's `ZISO_IDX_MAX_ENTRIES`.
const INDEX_WINDOW: u64 = 257;

#[derive(Debug, thiserror::Error)]
pub enum ZsoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("not a ZSO file (no ZISO magic)")]
    NotZso,
    #[error("OPL cannot read this ZSO: {0}")]
    Unsupported(ZsoProblem),
    #[error("block {block} is damaged: {reason}")]
    Corrupt { block: u64, reason: &'static str },
    #[error("image size must be a positive multiple of 2048 bytes, got {0}")]
    BadImageSize(u64),
    #[error("stopped")]
    Stopped,
}

/// A header field OPL ignores or cannot handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ZsoProblem {
    /// OPL still reads the index at byte 24; ziso.py refuses the file.
    #[error("header size is {0}, not 24")]
    HeaderSize(u32),
    #[error("block size is {0}; OPL only reads 2048-byte blocks")]
    BlockSize(u32),
    #[error("version {0} is newer than 1")]
    Version(u8),
    #[error("index shift {0} is larger than {MAX_ALIGN}")]
    Align(u8),
    #[error("uncompressed size {0} is not a multiple of 2048; the tail is unreadable")]
    PartialBlock(u64),
    #[error("uncompressed size is 0")]
    Empty,
}

impl ZsoProblem {
    /// Problems that stop OPL from reading the image at all.
    pub fn is_fatal(self) -> bool {
        matches!(self, Self::BlockSize(_) | Self::Align(_) | Self::Empty)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZsoHeader {
    pub header_size: u32,
    /// Size of the original ISO in bytes.
    pub total_bytes: u64,
    pub block_size: u32,
    pub version: u8,
    pub align: u8,
    pub reserved: [u8; 2],
}

impl ZsoHeader {
    /// Returns `None` without the `ZISO` magic.
    pub fn parse(bytes: &[u8; HEADER_SIZE]) -> Option<Self> {
        if bytes[..4] != MAGIC {
            return None;
        }
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let mut total = [0u8; 8];
        total.copy_from_slice(&bytes[8..16]);
        Some(Self {
            header_size: u32_at(4),
            total_bytes: u64::from_le_bytes(total),
            block_size: u32_at(16),
            version: bytes[20],
            align: bytes[21],
            reserved: [bytes[22], bytes[23]],
        })
    }

    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[..4].copy_from_slice(&MAGIC);
        out[4..8].copy_from_slice(&self.header_size.to_le_bytes());
        out[8..16].copy_from_slice(&self.total_bytes.to_le_bytes());
        out[16..20].copy_from_slice(&self.block_size.to_le_bytes());
        out[20] = self.version;
        out[21] = self.align;
        out[22..].copy_from_slice(&self.reserved);
        out
    }

    /// Blocks OPL reads: `total_bytes >> 11`, kept to 32 bits as in `ziso_init()`.
    pub fn block_count(&self) -> u64 {
        (self.total_bytes >> 11) & u64::from(u32::MAX)
    }

    pub fn problems(&self) -> Vec<ZsoProblem> {
        let mut found = Vec::new();
        if self.header_size != HEADER_SIZE as u32 {
            found.push(ZsoProblem::HeaderSize(self.header_size));
        }
        if self.block_size != BLOCK_SIZE {
            found.push(ZsoProblem::BlockSize(self.block_size));
        }
        if self.version > 1 {
            found.push(ZsoProblem::Version(self.version));
        }
        if self.align > MAX_ALIGN {
            found.push(ZsoProblem::Align(self.align));
        }
        if self.total_bytes == 0 {
            found.push(ZsoProblem::Empty);
        } else if !self.total_bytes.is_multiple_of(u64::from(BLOCK_SIZE)) {
            found.push(ZsoProblem::PartialBlock(self.total_bytes));
        }
        found
    }
}

/// Reads sectors out of a ZSO file, as OPL's isofs does.
#[derive(Debug)]
pub struct ZsoReader<R> {
    inner: R,
    header: ZsoHeader,
    window_start: u64,
    window: Vec<u32>,
    block: Vec<u8>,
}

impl<R: Read + Seek> ZsoReader<R> {
    pub fn new(mut inner: R) -> Result<Self, ZsoError> {
        let mut bytes = [0u8; HEADER_SIZE];
        inner.seek(SeekFrom::Start(0))?;
        inner
            .read_exact(&mut bytes)
            .map_err(|err| match err.kind() {
                io::ErrorKind::UnexpectedEof => ZsoError::NotZso,
                _ => ZsoError::Io(err),
            })?;
        let header = ZsoHeader::parse(&bytes).ok_or(ZsoError::NotZso)?;
        if let Some(&fatal) = header.problems().iter().find(|p| p.is_fatal()) {
            return Err(ZsoError::Unsupported(fatal));
        }
        Ok(Self {
            inner,
            header,
            window_start: 0,
            window: Vec::new(),
            block: Vec::with_capacity(SECTOR_SIZE),
        })
    }

    pub fn header(&self) -> &ZsoHeader {
        &self.header
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    /// Index entries `n` and `n + 1`.
    fn entries(&mut self, n: u64) -> Result<(u32, u32), ZsoError> {
        let cached = n >= self.window_start && n + 1 < self.window_start + self.window.len() as u64;
        if !cached {
            let count = INDEX_WINDOW.min(self.header.block_count() + 1 - n);
            let mut raw = vec![0u8; usize::try_from(count * 4).unwrap_or(0)];
            self.inner
                .seek(SeekFrom::Start(HEADER_SIZE as u64 + n * 4))?;
            self.inner.read_exact(&mut raw)?;
            self.window = raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect();
            self.window_start = n;
        }
        let i = usize::try_from(n - self.window_start).unwrap_or(usize::MAX);
        match (self.window.get(i), self.window.get(i + 1)) {
            (Some(&a), Some(&b)) => Ok((a, b)),
            _ => Err(ZsoError::Corrupt {
                block: n,
                reason: "index is truncated",
            }),
        }
    }

    fn read_block(&mut self, n: u64, out: &mut Sector) -> Result<(), ZsoError> {
        let (entry, next) = self.entries(n)?;
        let align = u32::from(self.header.align);
        let start = u64::from(entry & OFFSET_MASK) << align;
        let end = u64::from(next & OFFSET_MASK) << align;
        let size = end.checked_sub(start).ok_or(ZsoError::Corrupt {
            block: n,
            reason: "index offsets go backwards",
        })?;
        // OPL reads at most one sector's worth; anything after is padding.
        let take = usize::try_from(size.min(SECTOR_SIZE as u64)).unwrap_or(SECTOR_SIZE);
        self.block.resize(take, 0);
        self.inner.seek(SeekFrom::Start(start))?;
        self.inner.read_exact(&mut self.block)?;
        if entry & PLAIN != 0 {
            if take != SECTOR_SIZE {
                return Err(ZsoError::Corrupt {
                    block: n,
                    reason: "uncompressed block is shorter than 2048 bytes",
                });
            }
            out.copy_from_slice(&self.block);
            return Ok(());
        }
        decompress_fast(&self.block, out).map_err(|reason| ZsoError::Corrupt { block: n, reason })
    }
}

impl<R: Read + Seek> SectorSource for ZsoReader<R> {
    fn sector_count(&self) -> u64 {
        self.header.block_count()
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        if lba >= self.sector_count() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("sector {lba} is past the end of the image"),
            ));
        }
        self.read_block(lba, buf).map_err(|err| match err {
            ZsoError::Io(err) => err,
            other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
        })
    }
}

/// The most blocks [`StreamDecoder`] accepts: 16 GiB of ISO, twice a DVD-9.
const STREAM_MAX_BLOCKS: u64 = 1 << 23;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Header,
    Index,
    Blocks,
}

/// Decodes the ISO sectors of a ZSO from its bytes in file order: a copy
/// of the file hashes the image it holds on the way (GAME-5). Blocks must
/// follow the index in order, as every writer lays them out (`ziso.py`,
/// maxcso, ours); a file that jumps back fails with
/// [`ZsoError::Corrupt`], and is then hashed by reading it whole.
#[derive(Debug)]
pub struct StreamDecoder {
    stage: Stage,
    /// File bytes consumed.
    pos: u64,
    header: Option<ZsoHeader>,
    /// The header, then the index, as they arrive.
    head: Vec<u8>,
    /// Index entries the header announces.
    entries: usize,
    index: Vec<u32>,
    /// The next block to decode.
    next: u64,
    block: Vec<u8>,
    sector: Box<Sector>,
}

impl Default for StreamDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamDecoder {
    pub fn new() -> Self {
        Self {
            stage: Stage::Header,
            pos: 0,
            header: None,
            head: Vec::with_capacity(HEADER_SIZE),
            entries: 0,
            index: Vec::new(),
            next: 0,
            block: Vec::with_capacity(SECTOR_SIZE),
            sector: Box::new([0; SECTOR_SIZE]),
        }
    }

    /// Sectors in the image, once the header has arrived.
    pub fn sector_count(&self) -> Option<u64> {
        self.header.map(|h| h.block_count())
    }

    /// Every sector was decoded; bytes after the last block are ignored.
    pub fn is_done(&self) -> bool {
        self.stage == Stage::Blocks && self.next == self.index.len() as u64 - 1
    }

    /// File offset and stored length of block `n`, as [`ZsoReader`] reads it.
    fn span(&self, n: usize) -> Result<(u64, usize, bool), ZsoError> {
        let corrupt = |reason| ZsoError::Corrupt {
            block: n as u64,
            reason,
        };
        let align = u32::from(self.header.map_or(0, |h| h.align));
        let (entry, next) = (self.index[n], self.index[n + 1]);
        let start = u64::from(entry & OFFSET_MASK) << align;
        let end = u64::from(next & OFFSET_MASK) << align;
        let size = end
            .checked_sub(start)
            .ok_or_else(|| corrupt("index offsets go backwards"))?;
        let take = usize::try_from(size.min(SECTOR_SIZE as u64)).unwrap_or(SECTOR_SIZE);
        Ok((start, take, entry & PLAIN != 0))
    }

    /// Takes the next `bytes` of the file; `out` gets every sector they
    /// complete, in order.
    pub fn push(&mut self, mut bytes: &[u8], out: &mut dyn FnMut(&Sector)) -> Result<(), ZsoError> {
        while !bytes.is_empty() {
            match self.stage {
                Stage::Header | Stage::Index => {
                    let want = match self.header {
                        None => HEADER_SIZE,
                        Some(_) => self.entries * 4,
                    };
                    let n = (want - self.head.len()).min(bytes.len());
                    self.head.extend_from_slice(&bytes[..n]);
                    self.pos += n as u64;
                    bytes = &bytes[n..];
                    if self.head.len() < want {
                        continue;
                    }
                    if self.header.is_none() {
                        let mut raw = [0u8; HEADER_SIZE];
                        raw.copy_from_slice(&self.head);
                        let header = ZsoHeader::parse(&raw).ok_or(ZsoError::NotZso)?;
                        if let Some(&fatal) = header.problems().iter().find(|p| p.is_fatal()) {
                            return Err(ZsoError::Unsupported(fatal));
                        }
                        if header.block_count() > STREAM_MAX_BLOCKS {
                            return Err(ZsoError::BadImageSize(header.total_bytes));
                        }
                        // The index sits right after the 24 bytes, whatever
                        // the header says (`ziso_init()`).
                        let entries = usize::try_from(header.block_count() + 1)
                            .map_err(|_| ZsoError::BadImageSize(header.total_bytes))?;
                        self.header = Some(header);
                        self.entries = entries;
                        self.head = Vec::with_capacity(entries * 4);
                        self.stage = Stage::Index;
                    } else {
                        self.index = self
                            .head
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|c| u32::from_le_bytes(*c))
                            .collect();
                        self.head = Vec::new();
                        self.stage = Stage::Blocks;
                    }
                }
                Stage::Blocks => {
                    let n = usize::try_from(self.next).unwrap_or(usize::MAX);
                    if n + 1 >= self.index.len() {
                        // Padding or anything else after the last block.
                        self.pos += bytes.len() as u64;
                        return Ok(());
                    }
                    let (start, take, plain) = self.span(n)?;
                    if self.block.is_empty() && self.pos < start {
                        let skip = usize::try_from(start - self.pos)
                            .unwrap_or(usize::MAX)
                            .min(bytes.len());
                        self.pos += skip as u64;
                        bytes = &bytes[skip..];
                        continue;
                    }
                    if self.block.is_empty() && self.pos > start {
                        return Err(ZsoError::Corrupt {
                            block: self.next,
                            reason: "blocks are not stored in index order",
                        });
                    }
                    let n = (take - self.block.len()).min(bytes.len());
                    self.block.extend_from_slice(&bytes[..n]);
                    self.pos += n as u64;
                    bytes = &bytes[n..];
                    if self.block.len() < take {
                        continue;
                    }
                    self.decode(plain)?;
                    out(&self.sector);
                    self.block.clear();
                    self.next += 1;
                }
            }
        }
        // An image with no blocks is complete once its index is in.
        Ok(())
    }

    fn decode(&mut self, plain: bool) -> Result<(), ZsoError> {
        let block = self.next;
        if plain {
            if self.block.len() != SECTOR_SIZE {
                return Err(ZsoError::Corrupt {
                    block,
                    reason: "uncompressed block is shorter than 2048 bytes",
                });
            }
            self.sector.copy_from_slice(&self.block);
            return Ok(());
        }
        decompress_fast(&self.block, &mut self.sector)
            .map_err(|reason| ZsoError::Corrupt { block, reason })
    }
}

/// Raw LZ4 block decoding that stops once `out` is full, like
/// `LZ4_decompress_fast(src, dst, 2048)`: trailing input is ignored.
fn decompress_fast(input: &[u8], out: &mut Sector) -> Result<(), &'static str> {
    const TRUNCATED: &str = "compressed block is truncated";
    let byte = |i: usize| input.get(i).copied().ok_or(TRUNCATED);
    let length = |mut len: usize, ip: &mut usize| -> Result<usize, &'static str> {
        if len == 15 {
            loop {
                let b = byte(*ip)?;
                *ip += 1;
                len += usize::from(b);
                if b != 255 {
                    break;
                }
            }
        }
        Ok(len)
    };

    let (mut ip, mut op) = (0, 0);
    loop {
        let token = byte(ip)?;
        ip += 1;
        let literals = length(usize::from(token >> 4), &mut ip)?;
        let src = input.get(ip..ip + literals).ok_or(TRUNCATED)?;
        let dst = out
            .get_mut(op..op + literals)
            .ok_or("block decodes to more than 2048 bytes")?;
        dst.copy_from_slice(src);
        ip += literals;
        op += literals;
        if op == SECTOR_SIZE {
            return Ok(());
        }
        let offset = usize::from(u16::from_le_bytes([byte(ip)?, byte(ip + 1)?]));
        ip += 2;
        if offset == 0 || offset > op {
            return Err("match offset points before the block");
        }
        let len = length(usize::from(token & 0x0F), &mut ip)? + 4;
        if op + len > SECTOR_SIZE {
            return Err("block decodes to more than 2048 bytes");
        }
        // Byte by byte: the match may overlap its own output.
        for i in op..op + len {
            out[i] = out[i - offset];
        }
        op += len;
        if op == SECTOR_SIZE {
            return Err("block does not end with literals");
        }
    }
}

/// What [`compress`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZsoStats {
    pub blocks: u64,
    pub plain_blocks: u64,
    pub align: u8,
    /// Size of the ZSO file.
    pub output_bytes: u64,
}

/// Smallest index shift that keeps every offset within 31 bits, even if no
/// block compresses. (ziso.py uses `total // 2^31`, which can overflow by
/// the size of the index for incompressible images just under 2 GiB.)
pub fn align_for(total_bytes: u64) -> Result<u8, ZsoError> {
    let blocks = total_bytes / u64::from(BLOCK_SIZE);
    for align in 0..=MAX_ALIGN {
        let pad = (1u64 << align) - 1;
        let worst_end = HEADER_SIZE as u64 + 4 * (blocks + 1) + (blocks + 1) * pad + total_bytes;
        if worst_end >> align <= u64::from(OFFSET_MASK) {
            return Ok(align);
        }
    }
    Err(ZsoError::Unsupported(ZsoProblem::Align(MAX_ALIGN + 1)))
}

/// Compresses `total_bytes` of ISO data from `input` into `out`.
/// `progress` receives the number of input bytes processed so far.
pub fn compress<R: Read + ?Sized, W: Write + Seek + ?Sized>(
    input: &mut R,
    total_bytes: u64,
    out: &mut W,
    progress: &mut dyn FnMut(u64),
) -> Result<ZsoStats, ZsoError> {
    compress_with(input, total_bytes, out, &|| false, progress)
}

/// [`compress`] that checks `stop` between blocks and gives up with
/// [`ZsoError::Stopped`] (a cancelled conversion, GAME-4).
pub fn compress_with<R: Read + ?Sized, W: Write + Seek + ?Sized>(
    input: &mut R,
    total_bytes: u64,
    out: &mut W,
    stop: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> Result<ZsoStats, ZsoError> {
    let align = align_for(total_bytes)?;
    compress_aligned_with(input, total_bytes, out, align, stop, progress)
}

/// [`compress`] with an explicit index shift, at least [`align_for`] and at
/// most [`MAX_ALIGN`]. Larger shifts waste padding; tests use them.
pub fn compress_aligned<R: Read + ?Sized, W: Write + Seek + ?Sized>(
    input: &mut R,
    total_bytes: u64,
    out: &mut W,
    align: u8,
    progress: &mut dyn FnMut(u64),
) -> Result<ZsoStats, ZsoError> {
    compress_aligned_with(input, total_bytes, out, align, &|| false, progress)
}

/// The worst-case size of the ZSO of `total_bytes`: every block stored
/// plain, plus the header, the index and the alignment padding. What a
/// plan reserves before compressing (GAME-4).
pub fn max_output(total_bytes: u64) -> u64 {
    let blocks = total_bytes / u64::from(BLOCK_SIZE);
    let pad = align_for(total_bytes).map_or(0, |align| (1u64 << align) - 1);
    HEADER_SIZE as u64 + 4 * (blocks + 1) + (blocks + 1) * pad + total_bytes
}

fn compress_aligned_with<R: Read + ?Sized, W: Write + Seek + ?Sized>(
    input: &mut R,
    total_bytes: u64,
    out: &mut W,
    align: u8,
    stop: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> Result<ZsoStats, ZsoError> {
    if total_bytes == 0 || !total_bytes.is_multiple_of(u64::from(BLOCK_SIZE)) {
        return Err(ZsoError::BadImageSize(total_bytes));
    }
    if align > MAX_ALIGN || align < align_for(total_bytes)? {
        return Err(ZsoError::Unsupported(ZsoProblem::Align(align)));
    }
    let blocks = total_bytes / u64::from(BLOCK_SIZE);
    let header = ZsoHeader {
        header_size: HEADER_SIZE as u32,
        total_bytes,
        block_size: BLOCK_SIZE,
        version: 1,
        align,
        reserved: [0; 2],
    };
    let index_len = usize::try_from(blocks + 1).map_err(|_| ZsoError::BadImageSize(total_bytes))?;
    let mut index = vec![0u32; index_len];

    out.seek(SeekFrom::Start(0))?;
    out.write_all(&header.to_bytes())?;
    out.write_all(&vec![0u8; index_len * 4])?;
    let mut pos = (HEADER_SIZE + index_len * 4) as u64;

    let unit = 1u64 << align;
    let pad_to_alignment = |out: &mut W, pos: &mut u64| -> io::Result<()> {
        let pad = (unit - *pos % unit) % unit;
        out.write_all(&vec![0u8; usize::try_from(pad).unwrap_or(0)])?;
        *pos += pad;
        Ok(())
    };

    let mut sector = [0u8; SECTOR_SIZE];
    let mut plain_blocks = 0;
    for (n, slot) in index.iter_mut().take(index_len - 1).enumerate() {
        // Every 2 MiB: often enough to cancel at once, rare enough to be free.
        if n % 1024 == 0 && stop() {
            return Err(ZsoError::Stopped);
        }
        input.read_exact(&mut sector)?;
        pad_to_alignment(out, &mut pos)?;
        let offset = u32::try_from(pos >> align)
            .map_err(|_| ZsoError::Unsupported(ZsoProblem::Align(align)))?;
        let compressed = lz4_flex::block::compress(&sector);
        if compressed.len() * 100 >= COMPRESS_THRESHOLD_PERCENT * SECTOR_SIZE {
            out.write_all(&sector)?;
            pos += SECTOR_SIZE as u64;
            *slot = offset | PLAIN;
            plain_blocks += 1;
        } else {
            out.write_all(&compressed)?;
            pos += compressed.len() as u64;
            *slot = offset;
        }
        progress((n as u64 + 1) * u64::from(BLOCK_SIZE));
    }
    // Pad the end too, so the last block's size is exact.
    pad_to_alignment(out, &mut pos)?;
    index[index_len - 1] =
        u32::try_from(pos >> align).map_err(|_| ZsoError::Unsupported(ZsoProblem::Align(align)))?;

    out.seek(SeekFrom::Start(HEADER_SIZE as u64))?;
    let bytes: Vec<u8> = index.iter().flat_map(|e| e.to_le_bytes()).collect();
    out.write_all(&bytes)?;
    out.seek(SeekFrom::Start(pos))?;
    out.flush()?;

    Ok(ZsoStats {
        blocks,
        plain_blocks,
        align,
        output_bytes: pos,
    })
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Half compressible text, half pseudo-random noise.
    fn sample_iso(sectors: usize) -> Vec<u8> {
        let mut seed = 0x1234_5678u32;
        (0..sectors * SECTOR_SIZE)
            .map(|i| {
                if (i / SECTOR_SIZE).is_multiple_of(2) {
                    b"PLAYSTATION 2 "[i % 14]
                } else {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                }
            })
            .collect()
    }

    fn zso_of(iso: &[u8]) -> (Vec<u8>, ZsoStats) {
        let mut out = Cursor::new(Vec::new());
        let stats = compress(
            &mut Cursor::new(iso),
            iso.len() as u64,
            &mut out,
            &mut |_| {},
        )
        .unwrap();
        (out.into_inner(), stats)
    }

    fn decompress_all(zso: Vec<u8>) -> Vec<u8> {
        let mut reader = ZsoReader::new(Cursor::new(zso)).unwrap();
        let mut iso = Vec::new();
        let mut sector = [0u8; SECTOR_SIZE];
        for lba in 0..reader.sector_count() {
            reader.read_sector(lba, &mut sector).unwrap();
            iso.extend_from_slice(&sector);
        }
        iso
    }

    #[test]
    fn round_trip() {
        let iso = sample_iso(9);
        let (zso, stats) = zso_of(&iso);
        assert_eq!(stats.blocks, 9);
        assert_eq!(stats.plain_blocks, 4);
        assert_eq!(stats.align, 0);
        assert_eq!(stats.output_bytes, zso.len() as u64);
        assert!(zso.len() < iso.len());
        assert_eq!(decompress_all(zso), iso);
    }

    #[test]
    fn header_layout() {
        let (zso, _) = zso_of(&sample_iso(2));
        assert_eq!(&zso[..4], b"ZISO");
        assert_eq!(&zso[4..8], &24u32.to_le_bytes());
        assert_eq!(&zso[8..16], &(2u64 * 2048).to_le_bytes());
        assert_eq!(&zso[16..20], &2048u32.to_le_bytes());
        assert_eq!(&zso[20..24], &[1, 0, 0, 0]);
        let index: Vec<u32> = zso[24..36]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect();
        // Data starts right after header and 3 entries; block 1 is plain.
        assert_eq!(index[0], 36);
        assert_eq!(index[1] & PLAIN, PLAIN);
        assert_eq!(index[2] as usize, zso.len());
        let header = ZsoHeader::parse(zso[..24].try_into().unwrap()).unwrap();
        assert!(header.problems().is_empty());
        assert_eq!(header.to_bytes(), zso[..24]);
    }

    #[test]
    fn rejects_bad_sizes() {
        let mut out = Cursor::new(Vec::new());
        for size in [0, 100, 2049] {
            let err = compress(
                &mut Cursor::new(vec![0u8; 4096]),
                size,
                &mut out,
                &mut |_| {},
            )
            .unwrap_err();
            assert!(matches!(err, ZsoError::BadImageSize(s) if s == size));
        }
    }

    #[test]
    fn stops_and_bounds() {
        let iso = sample_iso(2048);
        let mut out = Cursor::new(Vec::new());
        let err = compress_with(
            &mut Cursor::new(&iso),
            iso.len() as u64,
            &mut out,
            &|| true,
            &mut |_| {},
        )
        .unwrap_err();
        assert!(matches!(err, ZsoError::Stopped));
        // The worst case covers incompressible data: every block stored.
        let mut seed = 0x9E37_79B9u32;
        let noise: Vec<u8> = (0..64 * SECTOR_SIZE)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let (zso, stats) = zso_of(&noise);
        assert_eq!(
            stats.plain_blocks, stats.blocks,
            "the noise must not compress"
        );
        assert!(zso.len() as u64 <= max_output(noise.len() as u64));
        assert!(max_output(4_700_372_992) > 4_700_372_992);
    }

    #[test]
    fn alignment() {
        assert_eq!(align_for(2048).unwrap(), 0);
        // Just under 2 GiB, but incompressible output would pass 2^31.
        assert_eq!(align_for((1 << 31) - 2048).unwrap(), 1);
        assert_eq!(align_for(4_700_372_992).unwrap(), 2);
        assert_eq!(align_for(8_547_991_552).unwrap(), 2);
    }

    fn header_with(f: impl FnOnce(&mut ZsoHeader)) -> Vec<u8> {
        let (mut zso, _) = zso_of(&sample_iso(2));
        let mut header = ZsoHeader::parse(zso[..24].try_into().unwrap()).unwrap();
        f(&mut header);
        zso[..24].copy_from_slice(&header.to_bytes());
        zso
    }

    #[test]
    fn opl_limits() {
        let err = ZsoReader::new(Cursor::new(header_with(|h| h.block_size = 4096))).unwrap_err();
        assert!(matches!(
            err,
            ZsoError::Unsupported(ZsoProblem::BlockSize(4096))
        ));
        let err = ZsoReader::new(Cursor::new(header_with(|h| h.align = 12))).unwrap_err();
        assert!(matches!(err, ZsoError::Unsupported(ZsoProblem::Align(12))));
        // OPL ignores these, so do we, but they are reported.
        let reader = ZsoReader::new(Cursor::new(header_with(|h| {
            h.header_size = 32;
            h.version = 2;
        })))
        .unwrap();
        assert_eq!(
            reader.header().problems(),
            [ZsoProblem::HeaderSize(32), ZsoProblem::Version(2)]
        );
        // A partial last block is unreadable, like in OPL.
        let reader =
            ZsoReader::new(Cursor::new(header_with(|h| h.total_bytes = 2048 + 100))).unwrap();
        assert_eq!(reader.sector_count(), 1);
        assert_eq!(reader.header().problems(), [ZsoProblem::PartialBlock(2148)]);
    }

    #[test]
    fn not_zso() {
        assert!(matches!(
            ZsoReader::new(Cursor::new(b"CISO".repeat(10))),
            Err(ZsoError::NotZso)
        ));
        assert!(matches!(
            ZsoReader::new(Cursor::new(b"ZISO".to_vec())),
            Err(ZsoError::NotZso)
        ));
    }

    #[test]
    fn reads_aligned_files_with_padding() {
        // Hand-built align=2 file: one compressed block followed by padding.
        let sector = [0x41u8; SECTOR_SIZE];
        let compressed = lz4_flex::block::compress(&sector);
        let header = ZsoHeader {
            header_size: 24,
            total_bytes: 2048,
            block_size: 2048,
            version: 1,
            align: 2,
            reserved: [0; 2],
        };
        let data_start = 32u32;
        let end = (data_start as usize + compressed.len()).div_ceil(4) * 4;
        let mut zso = header.to_bytes().to_vec();
        zso.extend_from_slice(&(data_start >> 2).to_le_bytes());
        zso.extend_from_slice(&((end as u32) >> 2).to_le_bytes());
        zso.extend_from_slice(&compressed);
        zso.resize(end, b'X');
        assert_eq!(decompress_all(zso), sector);
    }

    /// The ISO a [`StreamDecoder`] gets out of `zso` fed in `pieces`.
    fn stream_decode(zso: &[u8], pieces: &[usize]) -> Result<(Vec<u8>, bool), ZsoError> {
        let mut decoder = StreamDecoder::new();
        let mut iso = Vec::new();
        let mut at = 0;
        for size in pieces.iter().cycle() {
            if at == zso.len() {
                break;
            }
            let end = (at + size).min(zso.len());
            decoder.push(&zso[at..end], &mut |sector| iso.extend_from_slice(sector))?;
            at = end;
        }
        Ok((iso, decoder.is_done()))
    }

    #[test]
    fn stream_decoder_matches_the_reader() {
        let iso = sample_iso(11);
        for align in [0, 2, 5] {
            let mut out = Cursor::new(Vec::new());
            compress_aligned(
                &mut Cursor::new(&iso),
                iso.len() as u64,
                &mut out,
                align,
                &mut |_| {},
            )
            .unwrap();
            let zso = out.into_inner();
            for pieces in [&[1][..], &[7, 2048, 3], &[24, 48, 2049], &[1 << 20]] {
                let (decoded, done) = stream_decode(&zso, pieces).unwrap();
                assert!(done, "align {align}, pieces {pieces:?}");
                assert_eq!(decoded, iso, "align {align}, pieces {pieces:?}");
            }
            // Cut short: not done, and what came out is a prefix.
            let (part, done) = stream_decode(&zso[..zso.len() - 1], &[999]).unwrap();
            assert!(!done);
            assert_eq!(part, iso[..part.len()]);
        }
        // Trailing bytes after the last block are ignored.
        let (mut zso, _) = zso_of(&sample_iso(3));
        zso.extend_from_slice(b"trailing junk");
        let (decoded, done) = stream_decode(&zso, &[100]).unwrap();
        assert!(done);
        assert_eq!(decoded, sample_iso(3));
    }

    #[test]
    fn stream_decoder_refuses_what_it_cannot_follow() {
        assert!(matches!(
            stream_decode(&[0u8; 64], &[64]),
            Err(ZsoError::NotZso)
        ));
        // Blocks 0 and 1 swapped in the index: block 0 lies behind block 1.
        let (zso, _) = zso_of(&sample_iso(2));
        let mut swapped = zso.clone();
        let (a, b) = (24..28, 28..32);
        let first: Vec<u8> = swapped[a.clone()].to_vec();
        swapped.copy_within(b.clone(), a.start);
        swapped[b].copy_from_slice(&first);
        assert!(matches!(
            stream_decode(&swapped, &[4096]),
            Err(ZsoError::Corrupt { .. })
        ));
        // A damaged block is an error, as for the reader.
        let mut damaged = zso;
        let start = u32::from_le_bytes(damaged[24..28].try_into().unwrap()) as usize;
        damaged[start] = 0xFF;
        assert!(stream_decode(&damaged, &[4096]).is_err());
    }

    #[test]
    fn damaged_blocks() {
        let (mut zso, _) = zso_of(&sample_iso(2));
        // Make the end entry point back at block 0: block 1 now ends before
        // it starts.
        zso.copy_within(24..28, 32);
        let mut reader = ZsoReader::new(Cursor::new(zso)).unwrap();
        let mut sector = [0u8; SECTOR_SIZE];
        assert!(reader.read_sector(0, &mut sector).is_ok());
        assert!(reader.read_sector(1, &mut sector).is_err());
        assert!(reader.read_sector(2, &mut sector).is_err());
    }

    #[test]
    fn fast_decoder_matches_lz4_flex() {
        for iso in [
            sample_iso(1),
            vec![0u8; SECTOR_SIZE],
            (0..SECTOR_SIZE).map(|i| (i % 7) as u8).collect(),
        ] {
            let compressed = lz4_flex::block::compress(&iso);
            let mut out = [0u8; SECTOR_SIZE];
            decompress_fast(&compressed, &mut out).unwrap();
            assert_eq!(out[..], iso[..]);
            // Trailing bytes are ignored, truncation is not.
            let mut padded = compressed.clone();
            padded.extend_from_slice(b"XXX");
            decompress_fast(&padded, &mut out).unwrap();
            assert_eq!(out[..], iso[..]);
            assert!(decompress_fast(&compressed[..compressed.len() - 1], &mut out).is_err());
        }
    }
}
