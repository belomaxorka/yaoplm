//! Opening a disc image (ISO or ZSO) and getting its game ID the way OPL
//! does when it scans `CD/` and `DVD/` (IMP-2).
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.3 and §12. Mirrors
//! `ProbeZISO()` and `IsofsMount()` in `modules/isofs/isofs.c` and
//! `scanForISO()`/`GetStartupExecName()` in `src/supportbase.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997: the container is chosen by the
//! `ZISO` magic, not by the extension, and the ID comes from
//! `iso:/SYSTEM.CNF;1` (first 1023 bytes).

use std::io::{self, Read, Seek, SeekFrom};

use crate::iso9660::{
    self, DirRecord, IsoError, PVD_LBA, Pvd, RawImage, SYSTEM_CNF, Sector, SectorSource,
};
use crate::system_cnf::{self, CNF_READ_MAX, CnfError};
use crate::zso::{self, ZsoError, ZsoHeader, ZsoReader};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Iso,
    Zso,
}

#[derive(Debug)]
enum Source<R> {
    Raw(RawImage<R>),
    Zso(ZsoReader<R>),
}

/// An ISO or ZSO file as a sequence of sectors.
#[derive(Debug)]
pub struct DiscImage<R> {
    source: Source<R>,
}

impl<R: Read + Seek> DiscImage<R> {
    /// Detects ZSO by its magic, like OPL's `ProbeZISO()`.
    pub fn open(mut inner: R) -> Result<Self, ZsoError> {
        let mut magic = [0u8; 4];
        inner.seek(SeekFrom::Start(0))?;
        let is_zso = match inner.read_exact(&mut magic) {
            Ok(()) => magic == zso::MAGIC,
            Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => false,
            Err(err) => return Err(err.into()),
        };
        let source = if is_zso {
            Source::Zso(ZsoReader::new(inner)?)
        } else {
            Source::Raw(RawImage::new(inner)?)
        };
        Ok(Self { source })
    }

    pub fn container(&self) -> Container {
        match self.source {
            Source::Raw(_) => Container::Iso,
            Source::Zso(_) => Container::Zso,
        }
    }

    pub fn zso_header(&self) -> Option<&ZsoHeader> {
        match &self.source {
            Source::Zso(reader) => Some(reader.header()),
            Source::Raw(_) => None,
        }
    }
}

impl<R: Read + Seek> SectorSource for DiscImage<R> {
    fn sector_count(&self) -> u64 {
        match &self.source {
            Source::Raw(raw) => raw.sector_count(),
            Source::Zso(zso) => zso.sector_count(),
        }
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        match &mut self.source {
            Source::Raw(raw) => raw.read_sector(lba, buf),
            Source::Zso(zso) => zso.read_sector(lba, buf),
        }
    }
}

/// Why OPL would not list an image, or why we could not tell.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error(transparent)]
    Iso(#[from] IsoError),
    #[error("no SYSTEM.CNF;1 in the root directory (OPL needs exactly this name)")]
    NoSystemCnf,
    #[error(transparent)]
    Cnf(#[from] CnfError),
}

/// `SYSTEM.CNF` as OPL reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCnfFile {
    pub record: DirRecord,
    /// At most the first 1023 bytes.
    pub bytes: Vec<u8>,
}

/// Locates and reads `SYSTEM.CNF;1` through OPL's lookup.
pub fn read_system_cnf(
    src: &mut impl SectorSource,
    pvd: &Pvd,
) -> Result<Option<SystemCnfFile>, IsoError> {
    let Some(record) = iso9660::opl_find_root_file(src, pvd, SYSTEM_CNF)? else {
        return Ok(None);
    };
    let bytes = iso9660::read_file_prefix(src, &record, CNF_READ_MAX)?;
    Ok(Some(SystemCnfFile { record, bytes }))
}

/// The ID OPL derives for an image in `CD/` or `DVD/` with a new-style name.
pub fn opl_startup(src: &mut impl SectorSource) -> Result<String, StartupError> {
    let pvd = iso9660::read_pvd(src, PVD_LBA)?;
    let cnf = read_system_cnf(src, &pvd)?.ok_or(StartupError::NoSystemCnf)?;
    Ok(system_cnf::opl_startup(&cnf.bytes)?)
}
