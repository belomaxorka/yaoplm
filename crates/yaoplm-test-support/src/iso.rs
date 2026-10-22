//! A small ISO9660 writer for fixtures.
//!
//! Own code rather than a library, so tests can produce the images real
//! tools do not: names without `;1`, lower-case names, odd `SYSTEM.CNF`
//! contents, DVD-9 layer breaks, multi-gigabyte padding. The layout is plain
//! ISO9660 level 1: system area, PVD at 16, terminator at 17, L and M path
//! tables at 18 and 19, directories, then file data. Entries keep insertion
//! order (OPL does not care about sorting).

use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::content::{Content, resolve_seek};

pub const SECTOR: u64 = 2048;
const SECTOR_USIZE: usize = 2048;
/// 2026-10-22 00:00:00 UTC, for reproducible images.
const DIR_DATE: [u8; 7] = [126, 10, 22, 0, 0, 0, 0];
const VOLUME_DATE: &[u8; 17] = b"2026102200000000\0";

#[derive(Debug, Clone, Default)]
struct Dir {
    name: Vec<u8>,
    dirs: Vec<Dir>,
    files: Vec<(Vec<u8>, Content)>,
}

impl Dir {
    fn subdir(&mut self, name: &[u8]) -> &mut Dir {
        let index = match self.dirs.iter().position(|d| d.name == name) {
            Some(index) => index,
            None => {
                self.dirs.push(Dir {
                    name: name.to_vec(),
                    ..Dir::default()
                });
                self.dirs.len() - 1
            }
        };
        &mut self.dirs[index]
    }
}

/// Builds an ISO9660 image description; [`IsoBuilder::build`] turns it into
/// a [`VirtualIso`].
#[derive(Debug, Clone)]
pub struct IsoBuilder {
    system_id: String,
    volume_id: String,
    root: Dir,
    min_bytes: u64,
    layer0_sectors: Option<u64>,
}

impl Default for IsoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IsoBuilder {
    pub fn new() -> Self {
        Self {
            system_id: "PLAYSTATION".into(),
            volume_id: "YAOPLM_TEST".into(),
            root: Dir::default(),
            min_bytes: 0,
            layer0_sectors: None,
        }
    }

    pub fn volume_id(mut self, id: &str) -> Self {
        self.volume_id = id.to_owned();
        self
    }

    /// Adds a file. Directories are separated by `/`; every component is
    /// stored exactly as given, so include the `;1` version yourself.
    pub fn file(mut self, path: impl AsRef<[u8]>, content: Content) -> Self {
        let mut parts: Vec<&[u8]> = path.as_ref().split(|&b| b == b'/').collect();
        let name = parts.pop().unwrap_or_default().to_vec();
        let mut dir = &mut self.root;
        for part in parts {
            dir = dir.subdir(part);
        }
        dir.files.push((name, content));
        self
    }

    /// Adds `SYSTEM.CNF;1` with the given text.
    pub fn system_cnf(self, text: impl AsRef<[u8]>) -> Self {
        self.file("SYSTEM.CNF;1", Content::Bytes(text.as_ref().to_vec()))
    }

    /// Pads the image with zero sectors up to at least `bytes`.
    pub fn min_size(mut self, bytes: u64) -> Self {
        self.min_bytes = bytes;
        self
    }

    /// Makes a DVD-9 the way OPL detects one: the PVD's volume space size
    /// is `layer0_sectors + 16` and that sector holds layer 1's descriptor,
    /// so OPL computes `layer1_start = layer0_sectors`.
    pub fn dual_layer(mut self, layer0_sectors: u64) -> Self {
        self.layer0_sectors = Some(layer0_sectors);
        self
    }

    pub fn build(self) -> VirtualIso {
        Layout::new(&self).finish()
    }
}

struct PlacedDir<'a> {
    dir: &'a Dir,
    number: u16,
    parent: u16,
    lba: u64,
    sectors: u64,
    /// Sector of each subdirectory and file record target.
    child_dirs: Vec<usize>,
    file_lbas: Vec<u64>,
}

fn record_len(name_len: usize) -> usize {
    let len = 33 + name_len;
    len + len % 2
}

/// Sectors needed for a directory's records; records never cross sectors.
fn dir_sectors(dir: &Dir) -> u64 {
    let lens = [record_len(1), record_len(1)]
        .into_iter()
        .chain(dir.dirs.iter().map(|d| record_len(d.name.len())))
        .chain(dir.files.iter().map(|(n, _)| record_len(n.len())));
    let mut sectors = 1;
    let mut used = 0;
    for len in lens {
        if used + len > SECTOR_USIZE {
            sectors += 1;
            used = 0;
        }
        used += len;
    }
    sectors
}

struct Layout<'a> {
    builder: &'a IsoBuilder,
    dirs: Vec<PlacedDir<'a>>,
}

fn put_both16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
    buf[at + 2..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn put_both32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    buf[at + 4..at + 8].copy_from_slice(&v.to_be_bytes());
}

fn to_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn dir_record(lba: u64, size: u64, flags: u8, name: &[u8]) -> Vec<u8> {
    let len = record_len(name.len());
    let mut r = vec![0u8; len];
    r[0] = len as u8;
    put_both32(&mut r, 2, to_u32(lba));
    put_both32(&mut r, 10, to_u32(size));
    r[18..25].copy_from_slice(&DIR_DATE);
    r[25] = flags;
    put_both16(&mut r, 28, 1);
    r[32] = name.len() as u8;
    r[33..33 + name.len()].copy_from_slice(name);
    r
}

fn padded(text: &str, len: usize) -> Vec<u8> {
    let mut out = text.as_bytes().to_vec();
    out.resize(len, b' ');
    out
}

impl<'a> Layout<'a> {
    fn new(builder: &'a IsoBuilder) -> Self {
        // Breadth-first, as path tables require.
        let mut dirs = vec![PlacedDir {
            dir: &builder.root,
            number: 1,
            parent: 1,
            lba: 0,
            sectors: dir_sectors(&builder.root),
            child_dirs: Vec::new(),
            file_lbas: Vec::new(),
        }];
        let mut i = 0;
        while i < dirs.len() {
            let parent_dir = dirs[i].dir;
            for sub in &parent_dir.dirs {
                let index = dirs.len();
                dirs.push(PlacedDir {
                    dir: sub,
                    number: (index + 1) as u16,
                    parent: dirs[i].number,
                    lba: 0,
                    sectors: dir_sectors(sub),
                    child_dirs: Vec::new(),
                    file_lbas: Vec::new(),
                });
                dirs[i].child_dirs.push(index);
            }
            i += 1;
        }
        Self { builder, dirs }
    }

    fn path_table(&self, big_endian: bool) -> Vec<u8> {
        let mut table = Vec::new();
        for d in &self.dirs {
            let name: &[u8] = if d.number == 1 { &[0] } else { &d.dir.name };
            table.push(name.len() as u8);
            table.push(0);
            let lba = to_u32(d.lba);
            table.extend_from_slice(&if big_endian {
                lba.to_be_bytes()
            } else {
                lba.to_le_bytes()
            });
            table.extend_from_slice(&if big_endian {
                d.parent.to_be_bytes()
            } else {
                d.parent.to_le_bytes()
            });
            table.extend_from_slice(name);
            if name.len() % 2 == 1 {
                table.push(0);
            }
        }
        table
    }

    fn finish(mut self) -> VirtualIso {
        let mut next = 20u64;
        for d in &mut self.dirs {
            d.lba = next;
            next += d.sectors;
        }
        let meta_sectors = next;
        let mut extents = Vec::new();
        for d in &mut self.dirs {
            for (_, content) in &d.dir.files {
                d.file_lbas.push(next);
                extents.push((next, content.clone()));
                next += content.len().div_ceil(SECTOR);
            }
        }

        let b = self.builder;
        let mut total = next.max(b.min_bytes.div_ceil(SECTOR));
        let volume_space = match b.layer0_sectors {
            Some(layer0) => {
                assert!(next <= layer0 + 16, "file data overlaps the layer break");
                total = total.max(layer0 + 16 + 2);
                layer0 + 16
            }
            None => total,
        };

        let mut meta = vec![0u8; usize::try_from(meta_sectors * SECTOR).unwrap_or(0)];
        let root = &self.dirs[0];
        let pvd = self.pvd(volume_space, root.lba, root.sectors * SECTOR);
        meta[16 * SECTOR_USIZE..17 * SECTOR_USIZE].copy_from_slice(&pvd);
        let term = &mut meta[17 * SECTOR_USIZE..18 * SECTOR_USIZE];
        term[0] = 255;
        term[1..6].copy_from_slice(b"CD001");
        term[6] = 1;
        for (lba, big_endian) in [(18, false), (19, true)] {
            let table = self.path_table(big_endian);
            assert!(
                table.len() <= SECTOR_USIZE,
                "path table needs more than one sector"
            );
            let at = lba * SECTOR_USIZE;
            meta[at..at + table.len()].copy_from_slice(&table);
        }
        for d in &self.dirs {
            let parent = &self.dirs[usize::from(d.parent) - 1];
            let mut records = vec![
                dir_record(d.lba, d.sectors * SECTOR, 2, &[0]),
                dir_record(parent.lba, parent.sectors * SECTOR, 2, &[1]),
            ];
            for &child in &d.child_dirs {
                let c = &self.dirs[child];
                records.push(dir_record(c.lba, c.sectors * SECTOR, 2, &c.dir.name));
            }
            for ((name, content), &lba) in d.dir.files.iter().zip(&d.file_lbas) {
                records.push(dir_record(lba, content.len(), 0, name));
            }
            let mut at = usize::try_from(d.lba * SECTOR).unwrap_or(0);
            let mut used = 0;
            for r in records {
                if used + r.len() > SECTOR_USIZE {
                    at += SECTOR_USIZE - used;
                    used = 0;
                }
                meta[at..at + r.len()].copy_from_slice(&r);
                at += r.len();
                used += r.len();
            }
        }

        let extra = b
            .layer0_sectors
            .map(|layer0| (layer0 + 16, pvd.to_vec()))
            .into_iter()
            .collect();
        VirtualIso {
            meta,
            extents,
            extra,
            len: total * SECTOR,
            pos: 0,
        }
    }

    fn pvd(&self, volume_space: u64, root_lba: u64, root_size: u64) -> [u8; SECTOR_USIZE] {
        let b = self.builder;
        let mut s = [0u8; SECTOR_USIZE];
        s[0] = 1;
        s[1..6].copy_from_slice(b"CD001");
        s[6] = 1;
        s[8..40].copy_from_slice(&padded(&b.system_id, 32));
        s[40..72].copy_from_slice(&padded(&b.volume_id, 32));
        put_both32(&mut s, 80, to_u32(volume_space));
        put_both16(&mut s, 120, 1);
        put_both16(&mut s, 124, 1);
        put_both16(&mut s, 128, SECTOR as u16);
        put_both32(&mut s, 132, self.path_table(false).len() as u32);
        s[140..144].copy_from_slice(&18u32.to_le_bytes());
        s[148..152].copy_from_slice(&19u32.to_be_bytes());
        s[156..190].copy_from_slice(&dir_record(root_lba, root_size, 2, &[0]));
        for (start, len) in [
            (190, 128),
            (318, 128),
            (446, 128),
            (574, 128),
            (702, 37),
            (739, 37),
            (776, 37),
        ] {
            s[start..start + len].fill(b' ');
        }
        for start in [813, 830, 847, 864] {
            s[start..start + 17].copy_from_slice(VOLUME_DATE);
        }
        s[881] = 1;
        s
    }
}

/// An ISO image generated on the fly.
#[derive(Debug, Clone)]
pub struct VirtualIso {
    /// Sectors 0 up to the first file.
    meta: Vec<u8>,
    /// File data: start sector and content.
    extents: Vec<(u64, Content)>,
    /// Single sectors placed past the files (the DVD-9 layer-1 descriptor).
    extra: Vec<(u64, Vec<u8>)>,
    len: u64,
    pos: u64,
}

impl VirtualIso {
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn sector(&self, lba: u64, buf: &mut [u8; SECTOR_USIZE]) {
        buf.fill(0);
        let start = lba * SECTOR;
        if let Some(meta) = usize::try_from(start)
            .ok()
            .and_then(|at| self.meta.get(at..at + SECTOR_USIZE))
        {
            buf.copy_from_slice(meta);
            return;
        }
        if let Some((_, data)) = self.extra.iter().find(|(at, _)| *at == lba) {
            buf.copy_from_slice(data);
            return;
        }
        let i = self.extents.partition_point(|(first, _)| *first <= lba);
        if let Some((first, content)) = i.checked_sub(1).and_then(|i| self.extents.get(i)) {
            let offset = (lba - first) * SECTOR;
            if offset < content.len() {
                content.read_at(offset, buf);
            }
        }
    }

    /// The whole image in memory. Only for small fixtures.
    pub fn to_vec(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(usize::try_from(self.len).unwrap_or(0));
        let mut sector = [0u8; SECTOR_USIZE];
        for lba in 0..self.len / SECTOR {
            self.sector(lba, &mut sector);
            out.extend_from_slice(&sector);
        }
        out
    }

    /// Streams the image to `out`, sector by sector.
    pub fn write_to(&self, out: &mut impl Write) -> io::Result<u64> {
        let mut sector = [0u8; SECTOR_USIZE];
        for lba in 0..self.len / SECTOR {
            self.sector(lba, &mut sector);
            out.write_all(&sector)?;
        }
        Ok(self.len)
    }
}

impl Read for VirtualIso {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let mut sector = [0u8; SECTOR_USIZE];
        let mut done = 0;
        while done < buf.len() && self.pos < self.len {
            self.sector(self.pos / SECTOR, &mut sector);
            let within = (self.pos % SECTOR) as usize;
            let n = (SECTOR_USIZE - within).min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&sector[within..within + n]);
            done += n;
            self.pos += n as u64;
        }
        Ok(done)
    }
}

impl Seek for VirtualIso {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.pos = resolve_seek(self.pos, self.len, pos)?;
        Ok(self.pos)
    }
}
