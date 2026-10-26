//! The PS2 memory card file system, read only: OPL's `VMC/*.bin` and a
//! memory card emulator's `.mcd` images (VMC-2, LDR-5, ADR-0048).
//!
//! Format reference: `docs/research/02-opl-formats.md` §9,
//! `docs/research/15-cfg-vmc-prior-art.md` §2 and
//! `docs/research/19-psxmemcard-gen2-audit.md` §3. The structures follow
//! `MCDevInfo` and `McFsEntry` (ps2sdk `common/include/libmc-common.h`)
//! and the reader of mymcplusplus (`mymcplusplus/ps2mc.py`,
//! `ps2mc_dir.py`, GPL-3, Adubbz/mymcplusplus@3247c9c, after mymc by Ross
//! Ridge), whose logic this ports; nothing is copied from ps2sdk or OPL
//! (ADR-0002).
//!
//! - **Geometry comes from the superblock.** Page size, pages per cluster
//!   and per erase block, the card's clusters, the first allocatable
//!   cluster, where allocatable clusters end, the root directory's cluster
//!   and the indirect FAT clusters are read, never assumed: genvmc puts the
//!   indirect FAT at cluster 8 and Sony's formatter (and sd2psXtd) at 16.
//! - **Spare (ECC) bytes** follow each page in dumps of a real card (16 for
//!   a 512-byte page); emulators and OPL keep none. The file size tells.
//! - **The FAT is doubly indirect:** the superblock lists indirect FAT
//!   clusters; each holds the numbers of FAT clusters; each FAT cluster
//!   holds 32-bit entries. An entry's top bit says the cluster is in use;
//!   the rest names the next cluster of the chain; `0xFFFFFFFF` ends it.
//!   Chains and directory clusters count from the first allocatable
//!   cluster.
//! - **Directories** are files of 512-byte entries. A directory's own entry
//!   (and its `.` entry) gives the number of entries in `length`; a file's
//!   `length` is its size in bytes. Names are up to 32 bytes, NUL-padded.
//!
//! Every count and cluster number comes from the image, so each is bounded
//! before use: a chain longer than the card, a cluster past the card or a
//! directory claiming more entries than its clusters hold is an error, not
//! a hang or a panic.

use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom};

pub use crate::vmc::MAGIC;

/// `0x8000`: the entry is in use.
pub const MODE_EXISTS: u16 = 0x8000;
/// Hidden from the console's browser (`..`).
pub const MODE_HIDDEN: u16 = 0x2000;
/// A PS1 save kept on a PS2 card.
pub const MODE_PSX: u16 = 0x1000;
pub const MODE_POCKETSTATION: u16 = 0x0800;
pub const MODE_DIR: u16 = 0x0020;
pub const MODE_FILE: u16 = 0x0010;
/// Copy protected: the console's browser will not copy it.
pub const MODE_PROTECTED: u16 = 0x0008;

const ENTRY_SIZE: usize = 512;
const NAME_MAX: usize = 32;
const FAT_ALLOCATED: u32 = 0x8000_0000;
const FAT_END: u32 = 0xFFFF_FFFF;
const FAT_CLUSTER_MASK: u32 = 0x7FFF_FFFF;
const IFC_SLOTS: usize = 32;
/// The largest card any tool makes is 128 MiB (sd2psXtd's `CardSize`); a
/// million 1 KiB clusters is a gigabyte.
const MAX_CLUSTERS: u32 = 1 << 20;

/// Why an image is not a card this reader can open.
#[derive(Debug, thiserror::Error)]
pub enum CardError {
    #[error("not a PS2 memory card image (no superblock magic)")]
    NotACard,
    #[error("the superblock's geometry is impossible: {0}")]
    Geometry(&'static str),
    #[error(
        "the file is {file} bytes; the superblock describes {plain} (or {with_spare} with spare bytes)"
    )]
    Size {
        file: u64,
        plain: u64,
        with_spare: u64,
    },
    #[error("the root directory is damaged")]
    RootDamaged,
    #[error("cluster {0} is outside the card")]
    ClusterOutOfRange(u32),
    #[error("a chain of clusters starting at {0} loops or is longer than the card")]
    ChainTooLong(u32),
    #[error("{0} is too large to read ({1} bytes)")]
    TooLarge(String, u64),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Where things are on the card, from its superblock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Geometry {
    pub page_size: u32,
    pub pages_per_cluster: u32,
    pub pages_per_block: u32,
    pub clusters_per_card: u32,
    /// The first allocatable cluster.
    pub alloc_offset: u32,
    /// Allocatable clusters, counted from `alloc_offset`.
    pub alloc_end: u32,
    /// The root directory's first cluster, relative to `alloc_offset`.
    pub rootdir_cluster: u32,
    pub backup_block1: u32,
    pub backup_block2: u32,
    /// Indirect FAT clusters in use.
    pub ifc: Vec<u32>,
    pub card_type: u8,
    pub card_flags: u8,
    /// Bytes after each page: 0 in images without ECC.
    pub spare: u32,
}

impl Geometry {
    pub fn cluster_size(&self) -> u32 {
        self.page_size * self.pages_per_cluster
    }

    fn entries_per_cluster(&self) -> u32 {
        self.cluster_size() / 4
    }

    /// The card's size without spare bytes.
    pub fn card_bytes(&self) -> u64 {
        u64::from(self.clusters_per_card) * u64::from(self.cluster_size())
    }

    /// Reads and checks the superblock (the first 0x154 bytes and the
    /// card type at 0x150); `file_len` decides about spare bytes.
    pub fn parse(superblock: &[u8], file_len: u64) -> Result<Self, CardError> {
        if superblock.len() < 0x154 || !superblock.starts_with(MAGIC) {
            return Err(CardError::NotACard);
        }
        let u16_at = |i: usize| u32::from(u16::from_le_bytes([superblock[i], superblock[i + 1]]));
        let u32_at = |i: usize| {
            u32::from_le_bytes([
                superblock[i],
                superblock[i + 1],
                superblock[i + 2],
                superblock[i + 3],
            ])
        };
        let page_size = u16_at(0x28);
        let pages_per_cluster = u16_at(0x2A);
        let pages_per_block = u16_at(0x2C);
        let clusters_per_card = u32_at(0x30);
        let alloc_offset = u32_at(0x34);
        let alloc_end = u32_at(0x38);
        let rootdir_cluster = u32_at(0x3C);
        if !page_size.is_power_of_two() || !(128..=4096).contains(&page_size) {
            return Err(CardError::Geometry("page size"));
        }
        if !(1..=16).contains(&pages_per_cluster) {
            return Err(CardError::Geometry("pages per cluster"));
        }
        if page_size * pages_per_cluster < ENTRY_SIZE as u32 {
            return Err(CardError::Geometry(
                "cluster smaller than a directory entry",
            ));
        }
        if clusters_per_card == 0 || clusters_per_card > MAX_CLUSTERS {
            return Err(CardError::Geometry("clusters per card"));
        }
        if alloc_offset >= clusters_per_card {
            return Err(CardError::Geometry("first allocatable cluster"));
        }
        if alloc_end == 0 || alloc_end > clusters_per_card - alloc_offset {
            return Err(CardError::Geometry("allocatable clusters"));
        }
        if rootdir_cluster >= alloc_end {
            return Err(CardError::Geometry("root directory cluster"));
        }
        let mut ifc = Vec::new();
        for slot in 0..IFC_SLOTS {
            let cluster = u32_at(0x50 + slot * 4);
            // Unused slots are -1 (genvmc) or 0 past the last one (Sony).
            if cluster == FAT_END || (cluster == 0 && slot > 0) {
                break;
            }
            if cluster >= clusters_per_card {
                return Err(CardError::Geometry("indirect FAT cluster"));
            }
            ifc.push(cluster);
        }
        let mut geometry = Self {
            page_size,
            pages_per_cluster,
            pages_per_block,
            clusters_per_card,
            alloc_offset,
            alloc_end,
            rootdir_cluster,
            backup_block1: u32_at(0x40),
            backup_block2: u32_at(0x44),
            ifc,
            card_type: superblock[0x150],
            card_flags: superblock[0x151],
            spare: 0,
        };
        let epc = u64::from(geometry.entries_per_cluster());
        let fat_clusters = u64::from(alloc_end).div_ceil(epc);
        if (geometry.ifc.len() as u64) * epc < fat_clusters {
            return Err(CardError::Geometry("too few indirect FAT clusters"));
        }
        let plain = geometry.card_bytes();
        // mymc: `div_round_up(page_size, 128) * 4`.
        let spare = page_size.div_ceil(128) * 4;
        let with_spare = u64::from(clusters_per_card)
            * u64::from(pages_per_cluster)
            * u64::from(page_size + spare);
        if file_len == plain {
            geometry.spare = 0;
        } else if file_len == with_spare {
            geometry.spare = spare;
        } else {
            return Err(CardError::Size {
                file: file_len,
                plain,
                with_spare,
            });
        }
        Ok(geometry)
    }
}

/// A date as the console's clock writes it (Japan time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Tod {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
}

impl Tod {
    fn parse(bytes: &[u8]) -> Self {
        Self {
            second: bytes[1],
            minute: bytes[2],
            hour: bytes[3],
            day: bytes[4],
            month: bytes[5],
            year: u16::from_le_bytes([bytes[6], bytes[7]]),
        }
    }
}

/// One directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub mode: u16,
    /// Bytes of a file; entries of a directory.
    pub length: u32,
    pub created: Tod,
    /// First cluster, relative to the first allocatable cluster.
    pub cluster: u32,
    pub modified: Tod,
    pub attr: u32,
    /// As stored, without the NUL padding.
    pub name: Vec<u8>,
}

impl Entry {
    fn parse(bytes: &[u8]) -> Self {
        let u32_at =
            |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        let raw_name = &bytes[0x40..0x40 + NAME_MAX];
        let end = raw_name.iter().position(|&b| b == 0).unwrap_or(NAME_MAX);
        Self {
            mode: u16::from_le_bytes([bytes[0], bytes[1]]),
            length: u32_at(4),
            created: Tod::parse(&bytes[8..16]),
            cluster: u32_at(16),
            modified: Tod::parse(&bytes[24..32]),
            attr: u32_at(32),
            name: raw_name[..end].to_vec(),
        }
    }

    pub fn exists(&self) -> bool {
        self.mode & MODE_EXISTS != 0
    }

    pub fn is_dir(&self) -> bool {
        self.exists() && self.mode & MODE_DIR != 0
    }

    pub fn is_file(&self) -> bool {
        self.exists() && self.mode & (MODE_FILE | MODE_DIR) == MODE_FILE
    }

    /// `.` and `..`.
    pub fn is_dot(&self) -> bool {
        self.name == b"." || self.name == b".."
    }

    /// The name for display: names are ASCII in practice.
    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }
}

/// An open card image.
pub struct Card<R> {
    reader: R,
    geometry: Geometry,
    /// Indirect FAT clusters read so far (at most 32).
    ifc_cache: HashMap<u32, Vec<u32>>,
    /// The FAT cluster read last, by its number.
    fat_cache: Option<(u32, Vec<u32>)>,
}

impl<R: Read + Seek> Card<R> {
    /// Opens the image read by `reader`, `len` bytes long, and checks that
    /// its root directory starts with `.` and `..`.
    pub fn open(mut reader: R, len: u64) -> Result<Self, CardError> {
        let mut superblock = vec![0u8; 0x200];
        reader.seek(SeekFrom::Start(0))?;
        let read = read_up_to(&mut reader, &mut superblock)?;
        superblock.truncate(read);
        let geometry = Geometry::parse(&superblock, len)?;
        let mut card = Self {
            reader,
            geometry,
            ifc_cache: HashMap::new(),
            fat_cache: None,
        };
        let root = card.root_entries(2)?;
        if root.len() < 2 || root[0].name != b"." || root[1].name != b".." || !root[0].is_dir() {
            return Err(CardError::RootDamaged);
        }
        Ok(card)
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    /// Cluster `n` of the card, absolute.
    fn read_cluster(&mut self, n: u32) -> Result<Vec<u8>, CardError> {
        let g = &self.geometry;
        if n >= g.clusters_per_card {
            return Err(CardError::ClusterOutOfRange(n));
        }
        let page_size = u64::from(g.page_size);
        let raw_page = page_size + u64::from(g.spare);
        let first_page = u64::from(n) * u64::from(g.pages_per_cluster);
        let mut out = Vec::with_capacity(g.cluster_size() as usize);
        let mut page = vec![0u8; g.page_size as usize];
        for i in 0..u64::from(g.pages_per_cluster) {
            self.reader
                .seek(SeekFrom::Start((first_page + i) * raw_page))?;
            self.reader.read_exact(&mut page)?;
            out.extend_from_slice(&page);
        }
        Ok(out)
    }

    fn read_u32s(&mut self, cluster: u32) -> Result<Vec<u32>, CardError> {
        Ok(self
            .read_cluster(cluster)?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_le_bytes(*c))
            .collect())
    }

    /// The FAT entry of allocatable cluster `n`.
    fn fat(&mut self, n: u32) -> Result<u32, CardError> {
        let g = &self.geometry;
        if n >= g.alloc_end {
            return Err(CardError::ClusterOutOfRange(n));
        }
        let epc = g.entries_per_cluster();
        let fat_index = n / epc;
        let ifc = *g
            .ifc
            .get((fat_index / epc) as usize)
            .ok_or(CardError::ClusterOutOfRange(n))?;
        if !self.ifc_cache.contains_key(&ifc) {
            let entries = self.read_u32s(ifc)?;
            self.ifc_cache.insert(ifc, entries);
        }
        let fat_cluster = self
            .ifc_cache
            .get(&ifc)
            .and_then(|e| e.get((fat_index % epc) as usize))
            .copied()
            .ok_or(CardError::ClusterOutOfRange(n))?;
        if !matches!(&self.fat_cache, Some((cached, _)) if *cached == fat_cluster) {
            let entries = self.read_u32s(fat_cluster)?;
            self.fat_cache = Some((fat_cluster, entries));
        }
        self.fat_cache
            .as_ref()
            .and_then(|(_, entries)| entries.get((n % epc) as usize))
            .copied()
            .ok_or(CardError::ClusterOutOfRange(n))
    }

    /// The allocatable clusters of the chain starting at `first`, at most
    /// `limit` of them. A cluster marked free ends it, as in mymc.
    fn chain(&mut self, first: u32, limit: u32) -> Result<Vec<u32>, CardError> {
        let mut clusters = Vec::new();
        let mut seen = HashSet::new();
        let mut current = first;
        loop {
            if current >= self.geometry.alloc_end {
                return Err(CardError::ClusterOutOfRange(current));
            }
            if !seen.insert(current) {
                return Err(CardError::ChainTooLong(first));
            }
            clusters.push(current);
            if clusters.len() as u32 >= limit {
                return Ok(clusters);
            }
            let next = self.fat(current)?;
            if next == FAT_END || next & FAT_ALLOCATED == 0 {
                return Ok(clusters);
            }
            current = next & FAT_CLUSTER_MASK;
        }
    }

    /// Up to `count` entries of the directory whose first cluster is
    /// `first`.
    fn entries(&mut self, first: u32, count: u32) -> Result<Vec<Entry>, CardError> {
        let per_cluster = self.geometry.cluster_size() / ENTRY_SIZE as u32;
        let needed = count.div_ceil(per_cluster).max(1);
        if needed > self.geometry.alloc_end {
            return Err(CardError::ChainTooLong(first));
        }
        let mut entries = Vec::with_capacity(count as usize);
        for cluster in self.chain(first, needed)? {
            let bytes = self.read_cluster(cluster + self.geometry.alloc_offset)?;
            for raw in bytes.as_chunks::<ENTRY_SIZE>().0 {
                if entries.len() as u32 == count {
                    return Ok(entries);
                }
                entries.push(Entry::parse(raw));
            }
        }
        Ok(entries)
    }

    fn root_entries(&mut self, count: u32) -> Result<Vec<Entry>, CardError> {
        let first = self.geometry.rootdir_cluster;
        self.entries(first, count)
    }

    /// The root directory's entries, `.` and `..` included.
    pub fn root(&mut self) -> Result<Vec<Entry>, CardError> {
        let count = self.root_entries(1)?[0].length;
        self.root_entries(count)
    }

    /// The entries of directory `dir`, `.` and `..` included.
    pub fn list(&mut self, dir: &Entry) -> Result<Vec<Entry>, CardError> {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        self.entries(dir.cluster, dir.length)
    }

    /// The allocatable clusters an entry's data occupies, in chain order:
    /// as many as its length needs (a directory's entries, a file's bytes),
    /// fewer when its chain ends early.
    pub fn clusters(&mut self, entry: &Entry) -> Result<Vec<u32>, CardError> {
        let cluster_size = u64::from(self.geometry.cluster_size());
        let bytes = if entry.is_dir() {
            u64::from(entry.length) * ENTRY_SIZE as u64
        } else {
            u64::from(entry.length)
        };
        let needed = u32::try_from(bytes.div_ceil(cluster_size).max(1)).unwrap_or(u32::MAX);
        if needed > self.geometry.alloc_end {
            return Err(CardError::ChainTooLong(entry.cluster));
        }
        self.chain(entry.cluster, needed)
    }

    /// The bytes of file `file`, refused past `max` bytes.
    pub fn read(&mut self, file: &Entry, max: u64) -> Result<Vec<u8>, CardError> {
        let len = u64::from(file.length);
        if len > max {
            return Err(CardError::TooLarge(file.name_lossy(), len));
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        let cluster_size = u64::from(self.geometry.cluster_size());
        let needed = u32::try_from(len.div_ceil(cluster_size)).unwrap_or(u32::MAX);
        let clusters = self.chain(file.cluster, needed)?;
        let mut out = Vec::with_capacity(len as usize);
        for cluster in clusters {
            let bytes = self.read_cluster(cluster + self.geometry.alloc_offset)?;
            let take = (len - out.len() as u64).min(cluster_size) as usize;
            out.extend_from_slice(&bytes[..take]);
        }
        // A chain shorter than the length: what the card holds.
        Ok(out)
    }

    /// The entry at `path` (`APPS/OPNPS2LD.ELF`), names compared as the
    /// console does: exactly.
    pub fn find(&mut self, path: &str) -> Result<Option<Entry>, CardError> {
        let mut entries = self.root()?;
        let mut found = None;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let Some(entry) = entries
                .iter()
                .find(|e| e.exists() && !e.is_dot() && e.name == part.as_bytes())
                .cloned()
            else {
                return Ok(None);
            };
            entries = if entry.is_dir() {
                self.list(&entry)?
            } else {
                Vec::new()
            };
            found = Some(entry);
        }
        Ok(found)
    }

    /// Every entry below the root, depth first, with its path: what a card
    /// holds. Folders deeper than `depth` are not entered.
    pub fn walk(&mut self, depth: usize) -> Result<Vec<(String, Entry)>, CardError> {
        let mut out = Vec::new();
        let root = self.root()?;
        self.walk_into(&root, String::new(), depth, &mut out)?;
        Ok(out)
    }

    fn walk_into(
        &mut self,
        entries: &[Entry],
        prefix: String,
        depth: usize,
        out: &mut Vec<(String, Entry)>,
    ) -> Result<(), CardError> {
        for entry in entries.iter().filter(|e| e.exists() && !e.is_dot()) {
            let path = format!("{prefix}{}", entry.name_lossy());
            out.push((path.clone(), entry.clone()));
            if entry.is_dir() && depth > 0 {
                let inner = self.list(entry)?;
                self.walk_into(&inner, format!("{path}/"), depth - 1, out)?;
            }
        }
        Ok(())
    }
}

fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::vmc::{self, Layout, McTime};

    /// A blank card as OPL formats it.
    fn blank(size_mb: u32) -> Vec<u8> {
        let layout = Layout::new(size_mb).unwrap();
        let mut card = Vec::new();
        vmc::write_blank(&mut card, &layout, McTime::FALLBACK, &mut |_| true).unwrap();
        card
    }

    #[test]
    fn a_blank_card_holds_only_dot_and_dotdot() {
        for size in vmc::SIZES_MB {
            let image = blank(size);
            let len = image.len() as u64;
            let mut card = Card::open(Cursor::new(image), len).unwrap();
            let g = card.geometry().clone();
            assert_eq!((g.page_size, g.pages_per_cluster, g.spare), (512, 2, 0));
            assert_eq!(g.ifc, [8], "{size}");
            let root = card.root().unwrap();
            assert_eq!(root.len(), 2);
            assert!(root[0].is_dir() && root[1].mode & MODE_HIDDEN != 0);
            assert!(card.walk(4).unwrap().is_empty());
            assert_eq!(card.find("APPS").unwrap(), None);
        }
    }

    #[test]
    fn the_size_tells_whether_pages_carry_spare_bytes() {
        let image = blank(8);
        let sb = &image[..0x200];
        assert_eq!(Geometry::parse(sb, 8 << 20).unwrap().spare, 0);
        // 16384 pages of 512 + 16 bytes: a dump of a real card.
        assert_eq!(Geometry::parse(sb, 16384 * 528).unwrap().spare, 16);
        assert!(matches!(
            Geometry::parse(sb, 7 << 20),
            Err(CardError::Size { .. })
        ));
        assert!(matches!(
            Geometry::parse(&[0; 0x200], 8 << 20),
            Err(CardError::NotACard)
        ));
    }

    #[test]
    fn impossible_geometry_is_refused() {
        let image = blank(8);
        let set = |at: usize, bytes: &[u8]| {
            let mut sb = image[..0x200].to_vec();
            sb[at..at + bytes.len()].copy_from_slice(bytes);
            Geometry::parse(&sb, 8 << 20)
        };
        for (at, bytes) in [
            (0x28, &0u16.to_le_bytes()[..]),
            (0x28, &1000u16.to_le_bytes()[..]),
            (0x2A, &0u16.to_le_bytes()[..]),
            (0x2A, &17u16.to_le_bytes()[..]),
            (0x30, &0u32.to_le_bytes()[..]),
            (0x30, &u32::MAX.to_le_bytes()[..]),
            (0x34, &9000u32.to_le_bytes()[..]),
            (0x38, &u32::MAX.to_le_bytes()[..]),
            (0x3C, &9000u32.to_le_bytes()[..]),
            (0x50, &9000u32.to_le_bytes()[..]),
            (0x50, &u32::MAX.to_le_bytes()[..]),
        ] {
            assert!(
                matches!(set(at, bytes), Err(CardError::Geometry(_))),
                "{at:#x} {bytes:?}"
            );
        }
    }

    #[test]
    fn a_looping_or_broken_root_is_an_error_not_a_hang() {
        let layout = Layout::new(8).unwrap();
        let mut image = blank(8);
        let fat = layout.fat_start as usize * 1024;
        // The root directory claims 1000 entries and its cluster points at
        // itself.
        let root = layout.alloc_offset as usize * 1024;
        image[root + 4..root + 8].copy_from_slice(&1000u32.to_le_bytes());
        image[fat..fat + 4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        let mut card = Card::open(Cursor::new(image.clone()), image.len() as u64).unwrap();
        assert!(matches!(card.root(), Err(CardError::ChainTooLong(0))));
        // A chain into a cluster past the card.
        image[fat..fat + 4].copy_from_slice(&(0x8000_0000u32 | 0x7FFF_0000).to_le_bytes());
        let mut card = Card::open(Cursor::new(image.clone()), image.len() as u64).unwrap();
        assert!(matches!(card.root(), Err(CardError::ClusterOutOfRange(_))));
        // No `.` at the start of the root.
        image[root + 0x40] = b'x';
        assert!(matches!(
            Card::open(Cursor::new(image.clone()), image.len() as u64),
            Err(CardError::RootDamaged)
        ));
    }
}
