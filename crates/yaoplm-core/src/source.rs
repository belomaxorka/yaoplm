//! Import sources: ISO, ZSO, BIN/CUE and BIN files (IMP-1), and the CD/DVD
//! choice (IMP-3).
//!
//! Every source is read through the same sector code OPL's rules are
//! implemented on (`yaoplm_formats::image`, `cue`), so the game ID we
//! predict is the one OPL will show.
//!
//! CD or DVD, in this order (research/09 §2.6):
//! 1. a BIN/CUE dump is a CD (DVDs are never dumped as raw 2352-byte sectors);
//! 2. a second layer (DVD-9) means DVD;
//! 3. a UDF descriptor (`NSR02`/`NSR03`) in sectors 16-31 means DVD: PS2
//!    DVDs are UDF bridge discs, PS2 CDs are plain ISO9660;
//! 4. `CD-XA001` at byte 1024 of the primary volume descriptor means CD;
//! 5. otherwise a volume over 360,000 sectors (80-minute CD) is a DVD.
//!
//! The media byte is what OPL reports to the game (`sceCdGetDiskType`), and
//! it changes the emulated TOC and read speed (`modules/iopcore/cdvdman/
//! cdvdman.c`, `ncmd.c`, ps2homebrew/Open-PS2-Loader@b3a21997), so the
//! user can always override the guess.

use std::fmt;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use yaoplm_device::RelPath;
use yaoplm_device::fs::DeviceFs;
use yaoplm_device::source::{ReadSeek, SourceFs};
use yaoplm_formats::Media;
use yaoplm_formats::cue::{BinImage, CueSheet, TrackMode, probe_raw};
use yaoplm_formats::image::{self, DiscImage};
use yaoplm_formats::iso9660::{self, Layers, PVD_LBA, Pvd, SECTOR_SIZE, Sector, SectorSource};
use yaoplm_formats::system_cnf;
use yaoplm_formats::zso::ZsoError;

use crate::multipart::Joined;
use crate::plan::{Notice, Source, SourceFile, SourceKind};
use crate::scan::CD_MAX_BYTES;

/// What the user handed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFormat {
    Iso,
    Zso,
    /// A CUE sheet and its BIN.
    BinCue,
    /// A BIN without a sheet; the layout was detected.
    Bin,
    /// A CHD of a CD or a DVD (IMP-1b).
    Chd,
}

impl fmt::Display for InputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Iso => "ISO",
            Self::Zso => "ZSO",
            Self::BinCue => "BIN/CUE",
            Self::Bin => "BIN",
            Self::Chd => "CHD",
        })
    }
}

/// Facts that point to CD or DVD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MediaHints {
    pub raw_cd_dump: bool,
    pub dual_layer: bool,
    pub udf: bool,
    pub cd_xa: bool,
    pub volume_sectors: Option<u64>,
}

impl MediaHints {
    /// Reads the descriptors from an image whose PVD is known.
    pub fn read(src: &mut impl SectorSource, pvd: &Pvd, layers: Layers) -> Self {
        let mut sector: Sector = [0; SECTOR_SIZE];
        let cd_xa =
            src.read_sector(PVD_LBA, &mut sector).is_ok() && &sector[1024..1032] == b"CD-XA001";
        let udf = (PVD_LBA + 1..32).any(|lba| {
            src.read_sector(lba, &mut sector).is_ok()
                && matches!(&sector[1..6], b"NSR02" | b"NSR03")
        });
        Self {
            raw_cd_dump: false,
            dual_layer: matches!(layers, Layers::Dual { .. }),
            udf,
            cd_xa,
            volume_sectors: Some(u64::from(pvd.volume_space_size)),
        }
    }

    /// The media OPL should report, plus a warning when the hints disagree.
    pub fn suggest(&self, sectors: u64) -> (Media, Option<Notice>) {
        let volume = self.volume_sectors.unwrap_or(sectors);
        let large = volume * SECTOR_SIZE as u64 > CD_MAX_BYTES;
        let media = if self.raw_cd_dump {
            Media::Cd
        } else if self.dual_layer || self.udf {
            Media::Dvd
        } else if self.cd_xa {
            Media::Cd
        } else if large {
            Media::Dvd
        } else {
            Media::Cd
        };
        let conflict = (media == Media::Cd && large).then(|| {
            Notice::warning(
                "mediaConflict",
                format!(
                    "the image is marked as a CD but holds {volume} sectors, more than a CD; check CD or DVD"
                ),
            )
        });
        (media, conflict)
    }
}

/// Where the ISO sectors sit in a BIN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BinTrack {
    pub mode: TrackMode,
    /// `INDEX 01` of the data track.
    pub start: u64,
    /// The length of the track when more tracks follow it in the file.
    pub sectors: Option<u64>,
}

/// An import source, read and understood.
#[derive(Debug, Clone)]
pub struct Probed {
    pub input: PathBuf,
    pub format: InputFormat,
    /// The file holding the image bytes (the BIN for a sheet).
    pub data_path: PathBuf,
    pub files: Vec<SourceFile>,
    /// Size of `data_path`.
    pub data_len: u64,
    /// The ZISO magic, whatever the extension says.
    pub zso: bool,
    /// A CHD, read through its own reader.
    pub chd: bool,
    pub bin: Option<BinTrack>,
    /// Uncompressed image size.
    pub iso_len: u64,
    /// The ID OPL shows, or why OPL hides the disc.
    pub startup: Result<String, String>,
    /// `SYSTEM.CNF` has `BOOT` but no `BOOT2`.
    pub ps1: bool,
    pub hints: MediaHints,
    pub notices: Vec<Notice>,
}

impl Probed {
    /// The plan source that yields the image as ISO sectors.
    pub fn iso_source(&self) -> Source {
        let kind = match (self.bin, self.zso) {
            _ if self.chd => SourceKind::Chd,
            (Some(track), _) if track.mode != TrackMode::Mode1_2048 || track.start != 0 => {
                SourceKind::Bin {
                    sector_size: track.mode.sector_size(),
                    data_offset: track.mode.data_offset(),
                    first_sector: track.start,
                    sectors: self.iso_len / SECTOR_SIZE as u64,
                }
            }
            (_, true) => SourceKind::Zso,
            _ => SourceKind::Raw,
        };
        Source {
            path: self.data_path.clone(),
            kind,
            len: self.iso_len,
            files: self.files.clone(),
            on_device: false,
        }
    }

    /// The plan source that copies the file as it is (a ZSO stays a ZSO).
    pub fn raw_source(&self) -> Source {
        Source {
            path: self.data_path.clone(),
            kind: SourceKind::Raw,
            len: self.data_len,
            files: self.files.clone(),
            on_device: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("not a disc image YAOPLM can import")]
    NotAnImage,
    #[error("{0}")]
    Unsupported(String),
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn source_file(sources: &dyn SourceFs, path: &Path) -> Result<SourceFile, ProbeError> {
    let meta = sources.metadata(path).map_err(|source| ProbeError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(SourceFile {
        path: path.to_path_buf(),
        len: meta.len,
        modified: meta.modified,
    })
}

/// The BINs a sheet names, as far as they exist, even when the sheet
/// itself is refused: they belong to it and are not imported on their own.
pub fn sheet_files(sources: &dyn SourceFs, cue: &Path) -> Vec<PathBuf> {
    let mut head = Vec::new();
    if sources
        .open(cue)
        .and_then(|f| f.take(64 * 1024).read_to_end(&mut head))
        .is_err()
    {
        return Vec::new();
    }
    let Ok(sheet) = CueSheet::parse(&head) else {
        return Vec::new();
    };
    sheet
        .files
        .iter()
        .filter_map(|f| resolve_bin(sources, cue, &f.name))
        .collect()
}

/// The BIN a sheet names: relative to the sheet, else the same file name
/// next to it. Never anywhere else.
fn resolve_bin(sources: &dyn SourceFs, cue: &Path, name: &str) -> Option<PathBuf> {
    let dir = cue.parent().unwrap_or(Path::new(""));
    let relative = Path::new(name);
    let direct = dir.join(relative);
    let inside = relative.is_relative()
        && relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)));
    if inside && sources.metadata(&direct).is_ok_and(|m| !m.is_dir) {
        return Some(direct);
    }
    let base = Path::new(&name.replace('\\', "/"))
        .file_name()?
        .to_string_lossy()
        .to_lowercase();
    sources.list(dir).ok()?.into_iter().find(|p| {
        p.file_name()
            .is_some_and(|n| n.to_string_lossy().to_lowercase() == base)
            && sources.metadata(p).is_ok_and(|m| !m.is_dir)
    })
}

/// Reads `path` the way OPL would after the import.
pub fn probe(sources: &dyn SourceFs, path: &Path) -> Result<Probed, ProbeError> {
    let io_err = |p: &Path| {
        let p = p.to_path_buf();
        move |source| ProbeError::Io { path: p, source }
    };
    let ext = extension(path);
    let mut notices = Vec::new();
    let (format, data_path, files, bin) = match ext.as_str() {
        "iso" | "zso" | "chd" => {
            let file = source_file(sources, path)?;
            let format = match ext.as_str() {
                "zso" => InputFormat::Zso,
                "chd" => InputFormat::Chd,
                _ => InputFormat::Iso,
            };
            (format, path.to_path_buf(), vec![file], None)
        }
        "cue" => {
            let mut head = Vec::new();
            sources
                .open(path)
                .and_then(|f| f.take(64 * 1024).read_to_end(&mut head))
                .map_err(io_err(path))?;
            let sheet =
                CueSheet::parse(&head).map_err(|e| ProbeError::Unsupported(e.to_string()))?;
            let track = sheet
                .data_track()
                .map_err(|e| ProbeError::Unsupported(e.to_string()))?;
            let bin = resolve_bin(sources, path, &track.file).ok_or_else(|| {
                ProbeError::Unsupported(format!(
                    "the BIN named in the sheet ({}) is missing",
                    track.file
                ))
            })?;
            // The files of the other tracks belong to the dump: a move
            // deletes them too. The data file stays last.
            let mut files = vec![source_file(sources, path)?];
            for other in &track.other_files {
                if let Some(found) = resolve_bin(sources, path, other) {
                    files.push(source_file(sources, &found)?);
                }
            }
            files.push(source_file(sources, &bin)?);
            if !track.dropped.is_empty() {
                notices.push(audio_dropped(track.dropped.len()));
            }
            (
                InputFormat::BinCue,
                bin,
                files,
                Some(BinTrack {
                    mode: track.mode,
                    start: track.start_sector,
                    sectors: track.sectors,
                }),
            )
        }
        "bin" => {
            let mut head = Vec::new();
            sources
                .open(path)
                .and_then(|f| f.take(17 * 2352 + 64).read_to_end(&mut head))
                .map_err(io_err(path))?;
            let mode = probe_raw(&head).ok_or_else(|| {
                ProbeError::Unsupported(
                    "BIN without a CUE sheet, and no ISO9660 volume found in it".into(),
                )
            })?;
            notices.push(Notice::info(
                "binLayoutDetected",
                format!("no CUE sheet: the BIN was read as a single {mode} track"),
            ));
            let file = source_file(sources, path)?;
            (
                InputFormat::Bin,
                path.to_path_buf(),
                vec![file],
                Some(BinTrack {
                    mode,
                    start: 0,
                    sectors: None,
                }),
            )
        }
        _ => return Err(ProbeError::NotAnImage),
    };
    let data_len = files.last().map_or(0, |f| f.len);
    let packed = sources.in_archive(&data_path);

    let reader = sources.open(&data_path).map_err(io_err(&data_path))?;
    let mut chd_cd = false;
    let (mut src, zso): (Box<dyn SectorSource + Send>, bool) = match bin {
        Some(track) => (Box::new(bin_image(reader, track, data_len)?), false),
        None if format == InputFormat::Chd => {
            let (image, cd, dropped) = open_chd(reader, &data_path)?;
            chd_cd = cd;
            if dropped > 0 {
                notices.push(audio_dropped(dropped));
            }
            (image, false)
        }
        None => match DiscImage::open(reader) {
            Ok(image) => {
                let zso = image.zso_header().is_some();
                if let Some(header) = image.zso_header() {
                    notices.extend(
                        header
                            .problems()
                            .iter()
                            .map(|p| Notice::warning("zsoHeader", format!("ZSO header: {p}"))),
                    );
                }
                (Box::new(image), zso)
            }
            Err(ZsoError::Io(source)) => {
                return Err(ProbeError::Io {
                    path: data_path,
                    source,
                });
            }
            Err(err) => {
                return Err(ProbeError::Unsupported(format!(
                    "OPL cannot read this ZSO: {err}"
                )));
            }
        },
    };
    if zso && format == InputFormat::Iso {
        notices.push(Notice::warning(
            "containerMismatch",
            "the file is a ZSO although it is named .iso",
        ));
    }
    if packed {
        notices.push(Notice::info(
            "fromArchive",
            "read straight from the archive; nothing is unpacked to the computer first",
        ));
    }

    let iso_len = src.sector_count() * SECTOR_SIZE as u64;
    let pvd = iso9660::read_pvd(&mut src, PVD_LBA).ok();
    // Looking for a second layer reads the end of layer 0, which in an
    // archive means decoding the whole image; a disc that big is a DVD
    // anyway, and OPL finds the layer itself.
    let layers = match &pvd {
        Some(pvd) if !packed => iso9660::opl_layers(&mut src, pvd),
        _ => Layers::Single,
    };
    let mut hints = match &pvd {
        Some(pvd) => MediaHints::read(&mut src, pvd, layers),
        None => MediaHints::default(),
    };
    hints.raw_cd_dump = chd_cd || bin.is_some_and(|t| t.mode != TrackMode::Mode1_2048);
    let cnf = pvd
        .as_ref()
        .and_then(|pvd| image::read_system_cnf(&mut src, pvd).ok().flatten())
        .map(|f| system_cnf::parse_info(&f.bytes))
        .unwrap_or_default();
    let startup = image::opl_startup(&mut src).map_err(|e| e.to_string());
    let format = match (format, zso) {
        (InputFormat::Iso, true) => InputFormat::Zso,
        (format, _) => format,
    };
    Ok(Probed {
        input: path.to_path_buf(),
        format,
        data_path,
        files,
        data_len,
        zso,
        chd: format == InputFormat::Chd,
        bin,
        iso_len,
        ps1: cnf.boot2.is_none() && cnf.boot.is_some(),
        startup,
        hints,
        notices,
    })
}

fn audio_dropped(count: usize) -> Notice {
    Notice::warning(
        "audioTracksDropped",
        format!(
            "{count} audio tracks are left out: OPL plays a one-track disc, so music played from CD audio is missing"
        ),
    )
}

/// The ISO sectors of a BIN track.
fn bin_image(
    reader: Box<dyn ReadSeek>,
    track: BinTrack,
    data_len: u64,
) -> Result<BinImage<BufReader<Box<dyn ReadSeek>>>, ProbeError> {
    let reader = BufReader::with_capacity(1 << 20, reader);
    match track.sectors {
        Some(sectors) => {
            let end = (track.start + sectors) * u64::from(track.mode.sector_size());
            if end > data_len {
                return Err(ProbeError::Unsupported(
                    "the BIN ends before its data track does".into(),
                ));
            }
            Ok(BinImage::with_sectors(
                reader,
                track.mode,
                track.start,
                sectors,
            ))
        }
        None => BinImage::new(reader, track.mode, track.start, data_len)
            .map_err(|e| ProbeError::Unsupported(e.to_string())),
    }
}

/// A CHD as sectors, whether it is a CD, and how many tracks it drops.
#[cfg(feature = "chd")]
fn open_chd(
    reader: Box<dyn ReadSeek>,
    path: &Path,
) -> Result<(Box<dyn SectorSource + Send>, bool, usize), ProbeError> {
    use yaoplm_formats::chd::{ChdError, ChdImage, Layout};
    let image = ChdImage::open(reader).map_err(|e| match e {
        ChdError::Io(source) => ProbeError::Io {
            path: path.to_path_buf(),
            source,
        },
        other => ProbeError::Unsupported(other.to_string()),
    })?;
    let (cd, dropped) = match image.layout() {
        Layout::Cd { dropped, .. } => (true, dropped.len()),
        Layout::Dvd { .. } => (false, 0),
    };
    Ok((Box::new(image), cd, dropped))
}

#[cfg(not(feature = "chd"))]
fn open_chd(
    _reader: Box<dyn ReadSeek>,
    _path: &Path,
) -> Result<(Box<dyn SectorSource + Send>, bool, usize), ProbeError> {
    Err(ProbeError::Unsupported(
        "this build of YAOPLM reads no CHD images".into(),
    ))
}

/// Reads a [`SectorSource`] as a byte stream from a sector boundary.
struct SectorReader<S> {
    src: S,
    lba: u64,
    sector: Box<Sector>,
    /// Unread bytes at the end of `sector`.
    pending: usize,
}

impl<S: SectorSource> Read for SectorReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buf.len() {
            if self.pending == 0 {
                if self.lba >= self.src.sector_count() {
                    break;
                }
                self.src.read_sector(self.lba, &mut self.sector)?;
                self.lba += 1;
                self.pending = SECTOR_SIZE;
            }
            let from = SECTOR_SIZE - self.pending;
            let n = self.pending.min(buf.len() - written);
            buf[written..written + n].copy_from_slice(&self.sector[from..from + n]);
            self.pending -= n;
            written += n;
        }
        Ok(written)
    }
}

pub(crate) fn sector_stream<S: SectorSource + Send + 'static>(
    src: S,
    offset: u64,
) -> io::Result<Box<dyn Read + Send>> {
    if !offset.is_multiple_of(SECTOR_SIZE as u64) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "image streams start on a sector boundary",
        ));
    }
    Ok(Box::new(SectorReader {
        src,
        lba: offset / SECTOR_SIZE as u64,
        sector: Box::new([0; SECTOR_SIZE]),
        pending: 0,
    }))
}

/// The device path a source on the device keeps in a `PathBuf`.
pub(crate) fn device_path(path: &Path) -> io::Result<RelPath> {
    RelPath::new(&path.to_string_lossy())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

/// The bytes of `source` from `offset`, as a plan step writes them. A
/// source on the device (a conversion) is read through `fs`.
pub fn open_stream(
    sources: &dyn SourceFs,
    fs: &dyn DeviceFs,
    source: &Source,
    offset: u64,
) -> io::Result<Box<dyn Read + Send>> {
    let open = |path: &Path| -> io::Result<Box<dyn ReadSeek>> {
        if source.on_device {
            fs.open(&device_path(path)?)
        } else {
            sources.open(path)
        }
    };
    if source.kind == SourceKind::Joined {
        let parts = source
            .files
            .iter()
            .map(|file| Ok((open(&file.path)?, file.len)))
            .collect::<io::Result<Vec<_>>>()?;
        let mut joined = Joined::new(parts);
        joined.seek(SeekFrom::Start(offset))?;
        return Ok(Box::new(joined));
    }
    let mut reader: Box<dyn ReadSeek> = open(&source.path)?;
    match &source.kind {
        SourceKind::Raw => {
            reader.seek(SeekFrom::Start(offset))?;
            Ok(Box::new(reader))
        }
        SourceKind::Zso => {
            let image = DiscImage::open(reader).map_err(|e| match e {
                ZsoError::Io(err) => err,
                other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
            })?;
            sector_stream(image, offset)
        }
        SourceKind::Bin {
            sector_size,
            data_offset,
            first_sector,
            sectors,
        } => {
            let mode = TrackMode::from_layout(*sector_size, *data_offset).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "unknown BIN sector layout")
            })?;
            let image = BinImage::with_sectors(
                BufReader::with_capacity(1 << 20, reader),
                mode,
                *first_sector,
                *sectors,
            );
            sector_stream(image, offset)
        }
        SourceKind::Chd => {
            let (image, _, _) = open_chd(reader, &source.path).map_err(|e| match e {
                ProbeError::Io { source, .. } => source,
                other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
            })?;
            sector_stream(image, offset)
        }
        SourceKind::Joined => Err(io::Error::other("joined sources are opened above")),
    }
}
