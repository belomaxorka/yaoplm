//! The small part of ISO9660 OPL reads: the primary volume descriptor, the
//! root directory, `SYSTEM.CNF;1` and the DVD-9 layer break (IMP-2).
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.3 and §4.5. Mirrors
//! `ProbeISO9660()` and `cdvdman_locatefile()` in `modules/isofs/isofs.c`,
//! and `sbGetISO9660MaxLBA()`/`sbProbeISO9660()` in `src/supportbase.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997. RiptOPL, wOPL, OPL-Evolution and the
//! ps2-mmce fork use the same isofs code.
//!
//! OPL's reader is far stricter than the standard:
//! - Only sector 16 is checked (type 1 and `CD001`); there is no scan of the
//!   descriptor set and no Joliet.
//! - Names are compared with `strcmp` against the bytes at offset 33 of each
//!   record, so the root must hold exactly `SYSTEM.CNF;1` (upper case, with
//!   the version) followed by a zero byte (the padding byte).
//! - A record length of zero ends the sector; records starting at 2016 or
//!   later are not examined; a record with a name length of 0 hangs OPL.
//! - DVD-9: if sector `volume_space_size` holds another `\x01CD001`
//!   descriptor, layer 1 starts 16 sectors before it.
//!
//! [`read_dir`] is a lenient, standard reader for display and tests.

use std::io::{self, Read, Seek, SeekFrom};

pub const SECTOR_SIZE: usize = 2048;
/// Sector of the primary volume descriptor.
pub const PVD_LBA: u64 = 16;
/// The file OPL opens (`iso:/SYSTEM.CNF;1`).
pub const SYSTEM_CNF: &[u8] = b"SYSTEM.CNF;1";
/// `cdvdman_locatefile` stops examining a sector at this offset.
const OPL_SCAN_LIMIT: usize = 2016;

pub type Sector = [u8; SECTOR_SIZE];

/// Random access to the 2048-byte sectors of a disc image.
pub trait SectorSource {
    /// Whole sectors the image holds.
    fn sector_count(&self) -> u64;
    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()>;

    /// The whole 2352-byte CD sector behind the last `read_sector`, for
    /// sources that store one (a raw BIN track, a raw CHD track): Redump
    /// hashes CDs that way (ADR-0030).
    fn raw_sector(&self) -> Option<&[u8]> {
        None
    }
}

impl<T: SectorSource + ?Sized> SectorSource for Box<T> {
    fn sector_count(&self) -> u64 {
        (**self).sector_count()
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        (**self).read_sector(lba, buf)
    }

    fn raw_sector(&self) -> Option<&[u8]> {
        (**self).raw_sector()
    }
}

/// A plain `.iso` file.
#[derive(Debug)]
pub struct RawImage<R> {
    inner: R,
    len: u64,
}

impl<R: Read + Seek> RawImage<R> {
    pub fn new(mut inner: R) -> io::Result<Self> {
        let len = inner.seek(SeekFrom::End(0))?;
        Ok(Self { inner, len })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: Read + Seek> SectorSource for RawImage<R> {
    fn sector_count(&self) -> u64 {
        self.len / SECTOR_SIZE as u64
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        if lba >= self.sector_count() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("sector {lba} is past the end of the image"),
            ));
        }
        self.inner.seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))?;
        self.inner.read_exact(buf)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IsoError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("sector 16 holds no primary volume descriptor (not an ISO9660 image)")]
    NoPvd,
    #[error("a directory record with an empty name would hang OPL")]
    OplHang,
    #[error("the root directory layout sends OPL outside its sector buffer")]
    OplUndefined,
}

/// A directory record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirRecord {
    pub lba: u32,
    pub size: u32,
    pub flags: u8,
    /// Raw identifier, e.g. `SYSTEM.CNF;1`, `\0` for `.` and `\x01` for `..`.
    pub name: Vec<u8>,
}

impl DirRecord {
    pub fn is_dir(&self) -> bool {
        self.flags & 0x02 != 0
    }

    pub fn name_lossy(&self) -> String {
        String::from_utf8_lossy(&self.name).into_owned()
    }

    fn parse(bytes: &[u8]) -> Option<Self> {
        let len = usize::from(*bytes.first()?);
        let name_len = usize::from(*bytes.get(32)?);
        if len < 34 || 33 + name_len > len || len > bytes.len() {
            return None;
        }
        Some(Self {
            lba: le32(&bytes[2..6]),
            size: le32(&bytes[10..14]),
            flags: bytes[25],
            name: bytes[33..33 + name_len].to_vec(),
        })
    }
}

fn le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches([' ', '\0'])
        .to_owned()
}

/// The primary volume descriptor fields OPL and YAOPLM use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pvd {
    pub system_id: String,
    pub volume_id: String,
    /// Sectors in the volume (layer 0 for DVD-9). OPL's `maxLBA`.
    pub volume_space_size: u32,
    pub root: DirRecord,
}

fn is_pvd(sector: &Sector) -> bool {
    sector[0] == 1 && &sector[1..6] == b"CD001"
}

/// Reads the descriptor at `lba` the way `ProbeISO9660()` does.
pub fn read_pvd(src: &mut impl SectorSource, lba: u64) -> Result<Pvd, IsoError> {
    let mut sector = [0u8; SECTOR_SIZE];
    src.read_sector(lba, &mut sector)?;
    if !is_pvd(&sector) {
        return Err(IsoError::NoPvd);
    }
    // OPL takes the root location without validating the record.
    let root = &sector[156..190];
    Ok(Pvd {
        system_id: text(&sector[8..40]),
        volume_id: text(&sector[40..72]),
        volume_space_size: le32(&sector[80..84]),
        root: DirRecord {
            lba: le32(&root[2..6]),
            size: le32(&root[10..14]),
            flags: root[25],
            name: vec![root[33]],
        },
    })
}

/// Finds a file in the root directory exactly like `cdvdman_locatefile()`.
pub fn opl_find_root_file(
    src: &mut impl SectorSource,
    pvd: &Pvd,
    name: &[u8],
) -> Result<Option<DirRecord>, IsoError> {
    let mut sector = [0u8; SECTOR_SIZE];
    let mut lba = u64::from(pvd.root.lba);
    let mut remaining = i64::from(pvd.root.size);
    while remaining > 0 {
        // OPL ignores read errors here and scans a stale buffer; a truncated
        // image is an error for us.
        src.read_sector(lba, &mut sector)?;
        remaining -= SECTOR_SIZE as i64;
        lba += 1;

        let mut pos: usize = 0;
        loop {
            let Some(header) = sector.get(pos..pos + 33) else {
                return Err(IsoError::OplUndefined);
            };
            // `length` is a packed `short`: record length plus the
            // extended-attribute length byte, sign-extended.
            let length = i16::from_le_bytes([header[0], header[1]]);
            if length == 0 {
                break;
            }
            if header[32] == 0 {
                return Err(IsoError::OplHang);
            }
            if c_str_equals(&sector[pos + 33..], name)? {
                let record = DirRecord::parse(&sector[pos..]).ok_or(IsoError::OplUndefined)?;
                return Ok(Some(record));
            }
            pos = usize::try_from(pos as i64 + i64::from(length))
                .map_err(|_| IsoError::OplUndefined)?;
            if pos >= OPL_SCAN_LIMIT {
                break;
            }
        }
    }
    Ok(None)
}

/// `strcmp(name, bytes) == 0`, where running off the buffer is unknowable.
fn c_str_equals(bytes: &[u8], name: &[u8]) -> Result<bool, IsoError> {
    for (i, &want) in name.iter().chain(&[0]).enumerate() {
        match bytes.get(i) {
            Some(&got) if got == want => {}
            Some(_) => return Ok(false),
            None => return Err(IsoError::OplUndefined),
        }
    }
    Ok(true)
}

/// Reads the first `max` bytes of a file (OPL reads 1023 of `SYSTEM.CNF`).
pub fn read_file_prefix(
    src: &mut impl SectorSource,
    record: &DirRecord,
    max: usize,
) -> Result<Vec<u8>, IsoError> {
    let want = usize::try_from(record.size).unwrap_or(usize::MAX).min(max);
    let mut out = Vec::with_capacity(want);
    let mut sector = [0u8; SECTOR_SIZE];
    let mut lba = u64::from(record.lba);
    while out.len() < want {
        src.read_sector(lba, &mut sector)?;
        let take = (want - out.len()).min(SECTOR_SIZE);
        out.extend_from_slice(&sector[..take]);
        lba += 1;
    }
    Ok(out)
}

/// Lists a directory the standard way: records never cross sectors and a
/// zero length byte skips to the next sector.
pub fn read_dir(src: &mut impl SectorSource, dir: &DirRecord) -> Result<Vec<DirRecord>, IsoError> {
    let mut entries = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    let sectors = u64::from(dir.size).div_ceil(SECTOR_SIZE as u64);
    for i in 0..sectors {
        src.read_sector(u64::from(dir.lba) + i, &mut sector)?;
        let mut pos = 0;
        while pos < SECTOR_SIZE && sector[pos] != 0 {
            let Some(record) = DirRecord::parse(&sector[pos..]) else {
                break;
            };
            pos += usize::from(sector[pos]);
            entries.push(record);
        }
    }
    Ok(entries)
}

/// How OPL sees the layers of an image in `DVD/` (`bdmLaunchGame`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layers {
    Single,
    /// `layer1_start` as passed to cdvdman.
    Dual {
        layer1_start: u32,
    },
}

/// Probes for a second volume descriptor at `volume_space_size`.
/// OPL only does this for DVD media.
pub fn opl_layers(src: &mut impl SectorSource, pvd: &Pvd) -> Layers {
    let probe = u64::from(pvd.volume_space_size);
    let mut sector = [0u8; SECTOR_SIZE];
    match src.read_sector(probe, &mut sector) {
        Ok(()) if is_pvd(&sector) => Layers::Dual {
            layer1_start: pvd.volume_space_size.wrapping_sub(16),
        },
        _ => Layers::Single,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory image built sector by sector.
    struct Mem(Vec<u8>);

    impl SectorSource for Mem {
        fn sector_count(&self) -> u64 {
            (self.0.len() / SECTOR_SIZE) as u64
        }

        fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
            let start = usize::try_from(lba).unwrap() * SECTOR_SIZE;
            let src = self
                .0
                .get(start..start + SECTOR_SIZE)
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            buf.copy_from_slice(src);
            Ok(())
        }
    }

    fn record(lba: u32, size: u32, flags: u8, name: &[u8]) -> Vec<u8> {
        let mut len = 33 + name.len();
        len += len % 2;
        let mut r = vec![0u8; len];
        r[0] = len as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[6..10].copy_from_slice(&lba.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = flags;
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }

    /// PVD at 16, root directory at 18 holding `records`, file data at 20.
    fn image(records: &[Vec<u8>]) -> Mem {
        let mut img = vec![0u8; 24 * SECTOR_SIZE];
        let pvd = 16 * SECTOR_SIZE;
        img[pvd] = 1;
        img[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        img[pvd + 40..pvd + 47].copy_from_slice(b"TESTVOL");
        img[pvd + 80..pvd + 84].copy_from_slice(&24u32.to_le_bytes());
        let root = record(18, SECTOR_SIZE as u32, 2, &[0]);
        img[pvd + 156..pvd + 156 + root.len()].copy_from_slice(&root);
        let mut pos = 18 * SECTOR_SIZE;
        for r in records {
            img[pos..pos + r.len()].copy_from_slice(r);
            pos += r.len();
        }
        img[20 * SECTOR_SIZE..20 * SECTOR_SIZE + 5].copy_from_slice(b"hello");
        Mem(img)
    }

    fn dots() -> Vec<Vec<u8>> {
        vec![record(18, 2048, 2, &[0]), record(18, 2048, 2, &[1])]
    }

    fn find(records: Vec<Vec<u8>>) -> Result<Option<DirRecord>, IsoError> {
        let mut img = image(&records);
        let pvd = read_pvd(&mut img, PVD_LBA).unwrap();
        opl_find_root_file(&mut img, &pvd, SYSTEM_CNF)
    }

    #[test]
    fn finds_system_cnf_and_reads_it() {
        let mut records = dots();
        records.push(record(20, 5, 0, b"SLUS_200.62;1"));
        records.push(record(20, 5, 0, b"SYSTEM.CNF;1"));
        let mut img = image(&records);
        let pvd = read_pvd(&mut img, PVD_LBA).unwrap();
        assert_eq!(pvd.volume_id, "TESTVOL");
        assert_eq!(pvd.volume_space_size, 24);
        let found = opl_find_root_file(&mut img, &pvd, SYSTEM_CNF)
            .unwrap()
            .unwrap();
        assert_eq!(found.name, b"SYSTEM.CNF;1");
        assert_eq!(read_file_prefix(&mut img, &found, 1023).unwrap(), b"hello");
        assert_eq!(read_file_prefix(&mut img, &found, 3).unwrap(), b"hel");

        let names: Vec<String> = read_dir(&mut img, &pvd.root)
            .unwrap()
            .iter()
            .map(DirRecord::name_lossy)
            .collect();
        assert_eq!(names, ["\0", "\u{1}", "SLUS_200.62;1", "SYSTEM.CNF;1"]);
    }

    #[test]
    fn name_must_match_exactly() {
        for name in [
            &b"system.cnf;1"[..],
            b"SYSTEM.CNF",
            b"SYSTEM.CNF;2",
            b"SYSTEM.CNF;1X",
        ] {
            let mut records = dots();
            records.push(record(20, 5, 0, name));
            assert_eq!(
                find(records).unwrap(),
                None,
                "{:?}",
                String::from_utf8_lossy(name)
            );
        }
    }

    #[test]
    fn padding_byte_must_be_zero() {
        let mut records = dots();
        let mut r = record(20, 5, 0, b"SYSTEM.CNF;1");
        r[45] = b'X';
        records.push(r);
        assert_eq!(find(records).unwrap(), None);
    }

    #[test]
    fn empty_name_hangs_opl() {
        let mut records = dots();
        let mut r = record(20, 5, 0, b"A");
        r[32] = 0;
        records.push(r);
        records.push(record(20, 5, 0, b"SYSTEM.CNF;1"));
        assert!(matches!(find(records), Err(IsoError::OplHang)));
    }

    #[test]
    fn zero_length_ends_the_sector() {
        let mut records = dots();
        records.push(vec![0, 0]);
        records.push(record(20, 5, 0, b"SYSTEM.CNF;1"));
        assert_eq!(find(records).unwrap(), None);
    }

    #[test]
    fn searches_every_sector_of_the_root() {
        let mut img = image(&dots());
        // Grow the root to two sectors and put SYSTEM.CNF in the second.
        let root_size = 156 + 16 * SECTOR_SIZE + 10;
        img.0[root_size..root_size + 4].copy_from_slice(&4096u32.to_le_bytes());
        let r = record(20, 5, 0, b"SYSTEM.CNF;1");
        img.0[19 * SECTOR_SIZE..19 * SECTOR_SIZE + r.len()].copy_from_slice(&r);
        let pvd = read_pvd(&mut img, PVD_LBA).unwrap();
        assert_eq!(pvd.root.size, 4096);
        let found = opl_find_root_file(&mut img, &pvd, SYSTEM_CNF).unwrap();
        assert_eq!(found.map(|r| r.lba), Some(20));
    }

    #[test]
    fn no_pvd() {
        let mut img = Mem(vec![0u8; 20 * SECTOR_SIZE]);
        assert!(matches!(read_pvd(&mut img, PVD_LBA), Err(IsoError::NoPvd)));
        let mut short = Mem(vec![0u8; 4 * SECTOR_SIZE]);
        assert!(matches!(
            read_pvd(&mut short, PVD_LBA),
            Err(IsoError::Io(_))
        ));
    }

    #[test]
    fn layer_probe() {
        let mut img = image(&dots());
        let pvd = read_pvd(&mut img, PVD_LBA).unwrap();
        // Probe sector 24 is past the end.
        assert_eq!(opl_layers(&mut img, &pvd), Layers::Single);
        img.0.resize(25 * SECTOR_SIZE, 0);
        assert_eq!(opl_layers(&mut img, &pvd), Layers::Single);
        img.0[24 * SECTOR_SIZE] = 1;
        img.0[24 * SECTOR_SIZE + 1..24 * SECTOR_SIZE + 6].copy_from_slice(b"CD001");
        assert_eq!(opl_layers(&mut img, &pvd), Layers::Dual { layer1_start: 8 });
    }

    #[test]
    fn raw_image_reads_whole_sectors_only() {
        let mut data = vec![0u8; 2 * SECTOR_SIZE + 100];
        data[SECTOR_SIZE] = 7;
        let mut raw = RawImage::new(io::Cursor::new(data)).unwrap();
        assert_eq!(raw.sector_count(), 2);
        let mut buf = [0u8; SECTOR_SIZE];
        raw.read_sector(1, &mut buf).unwrap();
        assert_eq!(buf[0], 7);
        assert!(raw.read_sector(2, &mut buf).is_err());
    }
}
