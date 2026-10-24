//! Virtual memory cards `VMC/<name>.bin`: names, the validity check OPL
//! runs before using one, and blank cards formatted the way OPL's own VMC
//! menu formats them (VMC-1).
//!
//! Format reference: `docs/research/02-opl-formats.md` §9 and
//! `docs/research/15-cfg-vmc-prior-art.md` §2. Mirrors `vmc_superblock_t`
//! (`include/mcemu.h`), `sysCheckVMC()` and the `createParam` it fills
//! (`src/system.c`), `vmc_mcformat()` and `MCDevInfo`
//! (`modules/vmc/genvmc/genvmc.c`) and the 32-byte name buffers in
//! `src/guigame.c` of ps2homebrew/Open-PS2-Loader@b3a21997; directory
//! entries follow `McFsEntry` in ps2sdk `common/include/libmc-common.h`.
//! The logic is rewritten here, not copied (ADR-0002).
//!
//! OPL accepts a card when the superblock magic starts with
//! `Sony PS2 Memory Card Format`, the card type is 2, the file size equals
//! `pages_per_cluster * clusters_per_card * page_size`, and that size is a
//! whole number of MiB. A zero-filled file (what some managers write) fails
//! the check. On BDM the file must also be one contiguous cluster chain.
//!
//! A blank card as `vmc_mcformat()` writes it with OPL's block size of 16
//! pages: 1 KiB clusters of two 512-byte pages, no spare (ECC) bytes, N MiB
//! exactly, and 0xFF everywhere except
//! - cluster 0: the 384-byte superblock;
//! - cluster 8: the indirect FAT, the numbers of the FAT clusters, then
//!   zeros;
//! - the FAT clusters from 9: `0x7FFFFFFF` (free) for every allocatable
//!   cluster, `0xFFFFFFFF` (end of chain) for the root directory, zeros after
//!   the last entry. Allocatable clusters end before the two backup blocks,
//!   so a 64 MiB card's FAT fills only 255 of its 256 clusters: the last one
//!   keeps 0xFF;
//! - the root directory cluster right after the FAT: `.` and `..`.
//!
//! Times are what the console's clock would say: Japan time (UTC+9), which
//! the PS2's real-time clock keeps and memory card tools assume.

use std::io::{self, Write};

pub const VMC_FOLDER: &str = "VMC";
/// Name buffers are 32 bytes with a terminator.
pub const NAME_MAX: usize = 31;
/// Sizes OPL's VMC menu creates.
pub const SIZES_MB: [u32; 4] = [8, 16, 32, 64];
pub const MAGIC: &[u8] = b"Sony PS2 Memory Card Format";
/// Bytes of the superblock OPL reads.
pub const SUPERBLOCK_SIZE: usize = 0x200;
const MIB: u64 = 1 << 20;

/// `SUPERBLOCK_MAGIC` and `SUPERBLOCK_VERSION` in genvmc.
const FORMAT_MAGIC: &[u8; 28] = b"Sony PS2 Memory Card Format ";
const FORMAT_VERSION: &[u8] = b"1.2.0.0";
pub const PAGE_SIZE: u32 = 512;
pub const PAGES_PER_CLUSTER: u32 = 2;
pub const CLUSTER_SIZE: usize = 1024;
/// `createParam.VMC_blocksize` in `src/system.c`.
pub const PAGES_PER_BLOCK: u32 = 16;
const CLUSTERS_PER_BLOCK: u32 = PAGES_PER_BLOCK / PAGES_PER_CLUSTER;
const FAT_PER_CLUSTER: u32 = CLUSTER_SIZE as u32 / 4;
/// The superblock struct genvmc copies into cluster 0 (`MCDevInfo`).
const DEVINFO_SIZE: usize = 384;
/// One directory entry (`McFsEntry`).
const ENTRY_SIZE: usize = 512;
const FAT_FREE: u32 = 0x7FFF_FFFF;
const FAT_END: u32 = 0xFFFF_FFFF;
/// `sceMcFileAttrExists | sceMcFile0400 | sceMcFileAttrSubdir` with read,
/// write and execute: the `.` entry. `..` is hidden and not readable.
const MODE_DOT: u16 = 0x8000 | 0x0400 | 0x0020 | 0x0007;
const MODE_DOTDOT: u16 = 0x8000 | 0x2000 | 0x0400 | 0x0020 | 0x0006;
const MODE_EXISTS: u16 = 0x8000;
const MODE_SUBDIR: u16 = 0x0020;

/// Superblock fields OPL looks at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Superblock {
    /// `magic[40]`: the magic text followed by the version, e.g. `1.2.0.0`.
    pub magic: [u8; 40],
    pub page_size: u16,
    pub pages_per_cluster: u16,
    pub pages_per_block: u16,
    pub clusters_per_card: u32,
    /// First allocatable cluster; FAT and directory clusters count from it.
    pub alloc_offset: u32,
    /// The root directory's first cluster, relative to `alloc_offset`.
    pub rootdir_cluster: u32,
    pub card_type: u8,
    pub card_flags: u8,
}

impl Superblock {
    /// Reads the fields from the start of the file. Missing bytes count as 0,
    /// as OPL zeroes the struct before reading.
    pub fn parse(bytes: &[u8]) -> Self {
        let mut raw = [0u8; SUPERBLOCK_SIZE];
        let n = bytes.len().min(SUPERBLOCK_SIZE);
        raw[..n].copy_from_slice(&bytes[..n]);
        let u16_at = |i: usize| u16::from_le_bytes([raw[i], raw[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes([raw[i], raw[i + 1], raw[i + 2], raw[i + 3]]);
        let mut magic = [0u8; 40];
        magic.copy_from_slice(&raw[..40]);
        Self {
            magic,
            page_size: u16_at(0x28),
            pages_per_cluster: u16_at(0x2A),
            pages_per_block: u16_at(0x2C),
            clusters_per_card: u32_at(0x30),
            alloc_offset: u32_at(0x34),
            rootdir_cluster: u32_at(0x3C),
            card_type: raw[0x150],
            card_flags: raw[0x151],
        }
    }

    /// Where the root directory starts in a card without spare bytes.
    pub fn root_dir_offset(&self) -> u64 {
        (u64::from(self.alloc_offset) + u64::from(self.rootdir_cluster))
            * u64::from(self.pages_per_cluster)
            * u64::from(self.page_size)
    }

    /// Card size implied by the superblock.
    pub fn card_bytes(&self) -> u64 {
        u64::from(self.pages_per_cluster)
            * u64::from(self.clusters_per_card)
            * u64::from(self.page_size)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VmcProblem {
    #[error("not a PS2 memory card image (no superblock magic)")]
    BadMagic,
    #[error("card type {0} is not a PS2 card (2)")]
    CardType(u8),
    #[error("file is {file} bytes but the superblock describes {card}")]
    SizeMismatch { file: u64, card: u64 },
    #[error("size {0} is not a whole number of MiB")]
    NotWholeMib(u64),
}

/// `sysCheckVMC()`: the card size in MiB, or why OPL refuses the file.
pub fn opl_check(superblock: &[u8], file_size: u64) -> Result<u32, VmcProblem> {
    let sb = Superblock::parse(superblock);
    if !sb.magic.starts_with(MAGIC) {
        return Err(VmcProblem::BadMagic);
    }
    if sb.card_type != 2 {
        return Err(VmcProblem::CardType(sb.card_type));
    }
    if sb.card_bytes() != file_size {
        return Err(VmcProblem::SizeMismatch {
            file: file_size,
            card: sb.card_bytes(),
        });
    }
    if !file_size.is_multiple_of(MIB) {
        return Err(VmcProblem::NotWholeMib(file_size));
    }
    u32::try_from(file_size / MIB).map_err(|_| VmcProblem::NotWholeMib(file_size))
}

/// How many entries the directory starting with `entry` (its `.` entry)
/// holds: 2 on a blank card, one more per save. `None` when `entry` is not
/// a directory that exists.
pub fn directory_entries(entry: &[u8]) -> Option<u32> {
    let head = entry.get(..8)?;
    let mode = u16::from_le_bytes([head[0], head[1]]);
    (mode & MODE_EXISTS != 0 && mode & MODE_SUBDIR != 0)
        .then(|| u32::from_le_bytes([head[4], head[5], head[6], head[7]]))
}

/// A card size OPL's menu does not offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a VMC is 8, 16, 32 or 64 MiB, not {0}")]
pub struct VmcSizeError(pub u32);

/// Where `vmc_mcformat()` puts things on a blank card, in clusters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub size_mb: u32,
    pub clusters: u32,
    /// The indirect FAT clusters start at `blocksize / 2`.
    pub ifc_start: u32,
    pub ifc_clusters: u32,
    pub fat_start: u32,
    /// FAT clusters the indirect FAT lists.
    pub fat_clusters: u32,
    /// FAT clusters written: enough for `alloc_end` entries.
    pub fat_written: u32,
    pub alloc_offset: u32,
    /// Allocatable clusters, up to the two backup blocks.
    pub alloc_end: u32,
    pub backup_block1: u32,
    pub backup_block2: u32,
    pub max_allocatable: u32,
}

impl Layout {
    pub fn new(size_mb: u32) -> Result<Self, VmcSizeError> {
        if !SIZES_MB.contains(&size_mb) {
            return Err(VmcSizeError(size_mb));
        }
        let cluster = CLUSTER_SIZE as u32;
        let clusters = size_mb * 1024 * 1024 / cluster;
        let fat_clusters = (clusters * 4 - 1) / cluster + 1;
        // One for every size OPL makes; genvmc caps the list at 32.
        let ifc_clusters = ((fat_clusters * 4 - 1) / cluster + 1).min(32);
        let ifc_start = PAGES_PER_BLOCK / 2;
        let fat_start = ifc_start + ifc_clusters;
        let alloc_offset = fat_start + fat_clusters;
        let blocks = clusters / CLUSTERS_PER_BLOCK;
        let alloc_end = (blocks - 2) * CLUSTERS_PER_BLOCK - alloc_offset;
        Ok(Self {
            size_mb,
            clusters,
            ifc_start,
            ifc_clusters,
            fat_start,
            fat_clusters,
            fat_written: alloc_end.div_ceil(FAT_PER_CLUSTER),
            alloc_offset,
            alloc_end,
            backup_block1: blocks - 1,
            backup_block2: blocks - 2,
            // genvmc's fixed-point multiply by 0x10624DD3 works out to this.
            max_allocatable: clusters / 1000 * 1000 + 1,
        })
    }

    pub fn bytes(&self) -> u64 {
        u64::from(self.size_mb) * MIB
    }
}

/// A moment as the PS2's clock keeps it (`sceMcStDateTime`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct McTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub min: u8,
    pub sec: u8,
}

impl McTime {
    /// genvmc's time when the clock cannot be read.
    pub const FALLBACK: Self = Self {
        year: 2000,
        month: 3,
        day: 4,
        hour: 5,
        min: 6,
        sec: 7,
    };

    /// `unix` seconds in Japan time (UTC+9), the console's clock.
    pub fn from_unix(unix: i64) -> Self {
        let local = unix + 9 * 3600;
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        // Days to a civil date (Howard Hinnant's algorithm).
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        Self {
            year: u16::try_from(year).unwrap_or(u16::MAX),
            month: month as u8,
            day: day as u8,
            hour: (secs / 3600) as u8,
            min: (secs / 60 % 60) as u8,
            sec: (secs % 60) as u8,
        }
    }

    fn to_bytes(self) -> [u8; 8] {
        let [y0, y1] = self.year.to_le_bytes();
        [
            0, self.sec, self.min, self.hour, self.day, self.month, y0, y1,
        ]
    }
}

fn put_u16(buf: &mut [u8], at: usize, value: u16) {
    buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// Cluster 0: the superblock, then 0xFF.
fn superblock_cluster(layout: &Layout) -> [u8; CLUSTER_SIZE] {
    let mut buf = [0xFFu8; CLUSTER_SIZE];
    let sb = &mut buf[..DEVINFO_SIZE];
    // genvmc's MCDevInfo is static: every field it leaves alone is zero.
    sb.fill(0);
    sb[..28].copy_from_slice(FORMAT_MAGIC);
    sb[28..28 + FORMAT_VERSION.len()].copy_from_slice(FORMAT_VERSION);
    put_u16(sb, 0x28, PAGE_SIZE as u16);
    put_u16(sb, 0x2A, PAGES_PER_CLUSTER as u16);
    put_u16(sb, 0x2C, PAGES_PER_BLOCK as u16);
    put_u32(sb, 0x30, layout.clusters);
    put_u32(sb, 0x34, layout.alloc_offset);
    put_u32(sb, 0x38, layout.alloc_end);
    // 0x3C stays 0: the root directory is the first allocatable cluster.
    put_u32(sb, 0x40, layout.backup_block1);
    put_u32(sb, 0x44, layout.backup_block2);
    for slot in 0..32u32 {
        let ifc = if slot < layout.ifc_clusters {
            layout.ifc_start + slot
        } else {
            u32::MAX
        };
        put_u32(sb, 0x50 + 4 * slot as usize, ifc);
        // No bad blocks.
        put_u32(sb, 0xD0 + 4 * slot as usize, u32::MAX);
    }
    sb[0x150] = 2;
    sb[0x151] = 0x2B;
    put_u32(sb, 0x154, CLUSTER_SIZE as u32);
    put_u32(sb, 0x158, FAT_PER_CLUSTER);
    put_u32(sb, 0x15C, CLUSTERS_PER_BLOCK);
    // `cardform`: formatted.
    put_u32(sb, 0x160, 1);
    put_u32(sb, 0x170, layout.max_allocatable);
    put_u32(sb, 0x17C, u32::MAX);
    buf
}

/// Cluster `index` of a blank card.
pub fn blank_cluster(layout: &Layout, index: u32, time: McTime) -> [u8; CLUSTER_SIZE] {
    let mut buf = [0xFFu8; CLUSTER_SIZE];
    let per = FAT_PER_CLUSTER;
    if index == 0 {
        return superblock_cluster(layout);
    }
    if (layout.ifc_start..layout.fat_start).contains(&index) {
        let first = (index - layout.ifc_start) * per;
        for slot in 0..per {
            let fat = first + slot;
            let value = if fat < layout.fat_clusters {
                layout.fat_start + fat
            } else {
                0
            };
            put_u32(&mut buf, 4 * slot as usize, value);
        }
    } else if (layout.fat_start..layout.fat_start + layout.fat_written).contains(&index) {
        let first = (index - layout.fat_start) * per;
        for slot in 0..per {
            let value = match first + slot {
                0 => FAT_END,
                entry if entry < layout.alloc_end => FAT_FREE,
                _ => 0,
            };
            put_u32(&mut buf, 4 * slot as usize, value);
        }
    } else if index == layout.alloc_offset {
        buf.fill(0);
        let stamp = time.to_bytes();
        for (at, mode, name) in [(0, MODE_DOT, &b"."[..]), (ENTRY_SIZE, MODE_DOTDOT, b"..")] {
            let entry = &mut buf[at..at + ENTRY_SIZE];
            put_u16(entry, 0x00, mode);
            put_u32(entry, 0x04, 2);
            entry[0x08..0x10].copy_from_slice(&stamp);
            entry[0x18..0x20].copy_from_slice(&stamp);
            entry[0x40..0x40 + name.len()].copy_from_slice(name);
        }
    }
    buf
}

/// Writes a blank card of `layout` to `out`, cluster after cluster.
/// `progress(bytes)` is called every 64 KiB; returning false stops the
/// write with [`io::ErrorKind::Interrupted`].
pub fn write_blank(
    out: &mut dyn Write,
    layout: &Layout,
    time: McTime,
    progress: &mut dyn FnMut(u64) -> bool,
) -> io::Result<()> {
    const BATCH: u32 = 64;
    let mut buf = Vec::with_capacity(BATCH as usize * CLUSTER_SIZE);
    let mut index = 0;
    while index < layout.clusters {
        buf.clear();
        let end = (index + BATCH).min(layout.clusters);
        for cluster in index..end {
            buf.extend_from_slice(&blank_cluster(layout, cluster, time));
        }
        out.write_all(&buf)?;
        index = end;
        if !progress(u64::from(index) * CLUSTER_SIZE as u64) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "stopped"));
        }
    }
    Ok(())
}

/// A whole blank card in memory.
pub fn blank_card(size_mb: u32, time: McTime) -> Result<Vec<u8>, VmcSizeError> {
    let layout = Layout::new(size_mb)?;
    let mut out = Vec::with_capacity(CLUSTER_SIZE * layout.clusters as usize);
    // A Vec takes every write, and nothing stops this one.
    let _ = write_blank(&mut out, &layout, time, &mut |_| true);
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VmcNameError {
    #[error("VMC name is empty")]
    Empty,
    #[error("VMC name {0:?} is longer than {NAME_MAX} bytes")]
    TooLong(String),
    #[error("VMC name {0:?} contains characters a file name cannot hold")]
    BadChars(String),
}

/// Checks a name for `$VMC_0`/`$VMC_1`; the file is `VMC/<name>.bin`.
pub fn validate_name(name: &str) -> Result<(), VmcNameError> {
    if name.is_empty() {
        return Err(VmcNameError::Empty);
    }
    if name.len() > NAME_MAX {
        return Err(VmcNameError::TooLong(name.to_owned()));
    }
    let bad = name.chars().any(|c| {
        !c.is_ascii_graphic() && c != ' '
            || matches!(c, '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|')
    });
    if bad {
        return Err(VmcNameError::BadChars(name.to_owned()));
    }
    Ok(())
}

pub fn file_name(name: &str) -> String {
    format!("{name}.bin")
}

/// The name OPL's menu suggests: `<ID or group>_<slot>`, slot 0 or 1.
pub fn suggested_name(id: &str, slot: u8) -> String {
    format!("{id}_{slot}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn superblock(size_mb: u32) -> Vec<u8> {
        let mut sb = vec![0xFFu8; SUPERBLOCK_SIZE];
        sb[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
        sb[28..40].copy_from_slice(b"1.2.0.0\0\0\0\0\0");
        sb[0x28..0x2A].copy_from_slice(&512u16.to_le_bytes());
        sb[0x2A..0x2C].copy_from_slice(&2u16.to_le_bytes());
        sb[0x2C..0x2E].copy_from_slice(&16u16.to_le_bytes());
        sb[0x30..0x34].copy_from_slice(&(size_mb * 1024).to_le_bytes());
        sb[0x150] = 2;
        sb[0x151] = 0x2B;
        sb
    }

    #[test]
    fn accepts_genvmc_cards() {
        for mb in SIZES_MB {
            assert_eq!(opl_check(&superblock(mb), u64::from(mb) * MIB), Ok(mb));
        }
    }

    #[test]
    fn rejects_what_opl_rejects() {
        // A zero-filled "card", as some managers create.
        assert_eq!(
            opl_check(&[0u8; SUPERBLOCK_SIZE], 8 * MIB),
            Err(VmcProblem::BadMagic)
        );
        let mut sb = superblock(8);
        sb[0x150] = 1;
        assert_eq!(opl_check(&sb, 8 * MIB), Err(VmcProblem::CardType(1)));
        assert_eq!(
            opl_check(&superblock(8), 16 * MIB),
            Err(VmcProblem::SizeMismatch {
                file: 16 * MIB,
                card: 8 * MIB
            })
        );
        // A truncated file reads as zeros past its end.
        assert_eq!(
            opl_check(&superblock(8)[..0x100], 8 * MIB),
            Err(VmcProblem::CardType(0))
        );
    }

    fn u32_at(card: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(card[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn layout_matches_genvmc_arithmetic() {
        let eight = Layout::new(8).unwrap();
        assert_eq!(
            (eight.clusters, eight.fat_clusters, eight.fat_written),
            (8192, 32, 32)
        );
        assert_eq!(
            (eight.ifc_start, eight.fat_start, eight.alloc_offset),
            (8, 9, 41)
        );
        assert_eq!(eight.alloc_end, 1022 * 8 - 41);
        assert_eq!((eight.backup_block1, eight.backup_block2), (1023, 1022));
        assert_eq!(eight.max_allocatable, 8001);
        let big = Layout::new(64).unwrap();
        // The last FAT cluster is listed but never written.
        assert_eq!((big.fat_clusters, big.fat_written), (256, 255));
        assert_eq!(big.alloc_offset, 265);
        assert_eq!(big.max_allocatable, 65001);
        assert_eq!(Layout::new(4), Err(VmcSizeError(4)));
        assert_eq!(Layout::new(128), Err(VmcSizeError(128)));
    }

    #[test]
    fn blank_cards_pass_opls_check_and_read_back() {
        let time = McTime::from_unix(1_700_000_000);
        for mb in SIZES_MB {
            let card = blank_card(mb, time).unwrap();
            let layout = Layout::new(mb).unwrap();
            assert_eq!(card.len() as u64, u64::from(mb) * MIB);
            assert_eq!(
                opl_check(&card[..SUPERBLOCK_SIZE], card.len() as u64),
                Ok(mb)
            );
            let sb = Superblock::parse(&card);
            assert_eq!(&sb.magic[..28], FORMAT_MAGIC);
            assert_eq!(&sb.magic[28..40], b"1.2.0.0\0\0\0\0\0");
            assert_eq!(
                (sb.page_size, sb.pages_per_cluster, sb.pages_per_block),
                (512, 2, 16)
            );
            assert_eq!((sb.card_type, sb.card_flags), (2, 0x2B));
            assert_eq!(sb.alloc_offset, layout.alloc_offset);
            // genvmc's MCDevInfo, field by field past the common header.
            assert_eq!(u32_at(&card, 0x38), layout.alloc_end);
            assert_eq!(u32_at(&card, 0x50), 8);
            assert_eq!(u32_at(&card, 0x54), u32::MAX);
            assert_eq!(u32_at(&card, 0xD0), u32::MAX);
            assert_eq!(u32_at(&card, 0x158), 256);
            assert_eq!(u32_at(&card, 0x15C), 8);
            assert_eq!(u32_at(&card, 0x160), 1);
            assert_eq!(u32_at(&card, 0x17C), u32::MAX);
            assert!(card[DEVINFO_SIZE..CLUSTER_SIZE].iter().all(|&b| b == 0xFF));
            // The indirect FAT lists the FAT clusters.
            let ifc = 8 * CLUSTER_SIZE;
            for n in 0..layout.fat_clusters as usize {
                assert_eq!(u32_at(&card, ifc + 4 * n), 9 + n as u32);
            }
            // The FAT: root directory, then free clusters.
            let fat = 9 * CLUSTER_SIZE;
            assert_eq!(u32_at(&card, fat), FAT_END);
            assert_eq!(u32_at(&card, fat + 4), FAT_FREE);
            let last = fat + 4 * (layout.alloc_end as usize - 1);
            assert_eq!(u32_at(&card, last), FAT_FREE);
            if layout.fat_written < layout.fat_clusters {
                let unwritten = (9 + layout.fat_written as usize) * CLUSTER_SIZE;
                assert!(
                    card[unwritten..unwritten + CLUSTER_SIZE]
                        .iter()
                        .all(|&b| b == 0xFF)
                );
            } else {
                assert_eq!(u32_at(&card, last + 4), 0);
            }
            // The root directory: `.` and `..` only.
            let root = sb.root_dir_offset() as usize;
            assert_eq!(root, layout.alloc_offset as usize * CLUSTER_SIZE);
            assert_eq!(directory_entries(&card[root..]), Some(2));
            assert_eq!(&card[root..root + 2], &0x8427u16.to_le_bytes());
            assert_eq!(&card[root + 512..root + 514], &0xA426u16.to_le_bytes());
            assert_eq!(&card[root + 0x40..root + 0x42], b".\0");
            assert_eq!(&card[root + 512 + 0x40..root + 512 + 0x43], b"..\0");
            assert_eq!(&card[root + 8..root + 16], &time.to_bytes());
            // Everything after the root directory is erased.
            assert!(card[root + CLUSTER_SIZE..].iter().all(|&b| b == 0xFF));
        }
    }

    #[test]
    fn writing_can_be_stopped() {
        let layout = Layout::new(8).unwrap();
        let mut out = Vec::new();
        let err = write_blank(&mut out, &layout, McTime::FALLBACK, &mut |done| {
            done < 1 << 20
        })
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        assert_eq!(out.len(), 1 << 20);
    }

    #[test]
    fn console_clock() {
        // 2023-11-14 22:13:20 UTC is the next morning in Japan.
        assert_eq!(
            McTime::from_unix(1_700_000_000),
            McTime {
                year: 2023,
                month: 11,
                day: 15,
                hour: 7,
                min: 13,
                sec: 20
            }
        );
        assert_eq!(McTime::from_unix(0).hour, 9);
        assert_eq!(
            McTime::from_unix(951_782_400).to_bytes(),
            // 2000-02-29 00:00 UTC, a leap day.
            [0, 0, 0, 9, 29, 2, 0xD0, 0x07]
        );
    }

    #[test]
    fn directory_entry_counts() {
        assert_eq!(directory_entries(&[0x27, 0x84, 0, 0, 5, 0, 0, 0]), Some(5));
        // A file, or nothing at all.
        assert_eq!(directory_entries(&[0x17, 0x84, 0, 0, 5, 0, 0, 0]), None);
        assert_eq!(directory_entries(&[0xFF; 4]), None);
        assert_eq!(directory_entries(&[0; 8]), None);
    }

    #[test]
    fn names() {
        assert_eq!(suggested_name("SLUS_200.62", 0), "SLUS_200.62_0");
        assert_eq!(file_name("generic_1"), "generic_1.bin");
        assert!(validate_name("SLUS_200.62_0").is_ok());
        assert!(validate_name(&"a".repeat(31)).is_ok());
        assert!(matches!(
            validate_name(&"a".repeat(32)),
            Err(VmcNameError::TooLong(_))
        ));
        assert!(matches!(
            validate_name("a/b"),
            Err(VmcNameError::BadChars(_))
        ));
        assert!(matches!(
            validate_name("карта"),
            Err(VmcNameError::BadChars(_))
        ));
        assert_eq!(validate_name(""), Err(VmcNameError::Empty));
    }
}
