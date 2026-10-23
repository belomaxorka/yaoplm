//! CUE sheets and the raw sectors of a BIN track (IMP-1).
//!
//! OPL itself only reads 2048-byte ISO sectors and reports a disc with one
//! track (`modules/iopcore/cdvdman/ncmd.c`, ps2homebrew/Open-PS2-Loader@b3a21997),
//! so a BIN/CUE becomes an ISO by keeping the 2048 user-data bytes of every
//! sector. Rules from the prior art in research/09 §2 (hdl-dump `iin_cdrwin.c`,
//! ps2-opl-ripper `cd_sector.py`, rcheevos `cdreader.c`):
//! - user data starts at byte 0 (`MODE1/2048`), 16 (`MODE1/2352`), 24
//!   (`MODE2/2352`, Form 1) or 8 (`MODE2/2336`);
//! - track 1 is the data track (type `BINARY`); later tracks, audio in the
//!   PS2 discs that have them, are dropped and listed (IMP-1b): OPL
//!   emulates a one-track disc, so their music is lost either way. A second
//!   data track or a second session is refused, not dropped;
//! - several `FILE`s (Redump's `(Track 1).bin`, `(Track 2).bin`) and one
//!   file holding every track both work: in the latter the data track ends
//!   where the next track starts (its `INDEX 00`, else `INDEX 01`);
//! - every raw sector must carry the sync pattern and the mode the sheet
//!   names; Mode 2 Form 2 sectors (no ECC, 2324 bytes) cannot become ISO
//!   data and are refused;
//! - the track starts at its `INDEX 01`; `PREGAP` is not in the file.

use std::io::{self, Read, Seek, SeekFrom};

use crate::iso9660::{SECTOR_SIZE, Sector, SectorSource};

/// 74:59:74 at 75 frames per second; anything above is nonsense.
const MAX_FRAMES: u32 = 100 * 60 * 75;
const SYNC: [u8; 12] = [
    0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackMode {
    Mode1_2048,
    Mode1_2352,
    Mode2_2336,
    Mode2_2352,
}

impl TrackMode {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.to_ascii_uppercase().as_str() {
            "MODE1/2048" => Self::Mode1_2048,
            "MODE1/2352" => Self::Mode1_2352,
            "MODE2/2336" => Self::Mode2_2336,
            "MODE2/2352" => Self::Mode2_2352,
            _ => return None,
        })
    }

    pub fn sector_size(self) -> u32 {
        match self {
            Self::Mode1_2048 => 2048,
            Self::Mode2_2336 => 2336,
            Self::Mode1_2352 | Self::Mode2_2352 => 2352,
        }
    }

    /// Where the 2048 bytes of user data start in a raw sector.
    pub fn data_offset(self) -> u32 {
        match self {
            Self::Mode1_2048 => 0,
            Self::Mode2_2336 => 8,
            Self::Mode1_2352 => 16,
            Self::Mode2_2352 => 24,
        }
    }

    /// The mode from a sector size and data offset, as stored in plans.
    pub fn from_layout(sector_size: u32, data_offset: u32) -> Option<Self> {
        [
            Self::Mode1_2048,
            Self::Mode1_2352,
            Self::Mode2_2336,
            Self::Mode2_2352,
        ]
        .into_iter()
        .find(|m| m.sector_size() == sector_size && m.data_offset() == data_offset)
    }
}

impl std::fmt::Display for TrackMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mode1_2048 => "MODE1/2048",
            Self::Mode1_2352 => "MODE1/2352",
            Self::Mode2_2336 => "MODE2/2336",
            Self::Mode2_2352 => "MODE2/2352",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CueError {
    #[error("line {line}: {problem}")]
    Syntax { line: usize, problem: String },
    #[error("the sheet names no FILE")]
    NoFile,
    #[error("the sheet names no TRACK")]
    NoTrack,
    #[error("FILE type {0} is not supported; only BINARY")]
    FileType(String),
    #[error("track {0} is a second data track (mixed-mode or CD-Extra disc)")]
    SecondDataTrack(u32),
    #[error("track mode {0} holds no PS2 data")]
    NotData(String),
    #[error("the disc has several sessions")]
    MultiSession,
    #[error("the track has no INDEX 01")]
    NoIndex,
}

/// One `TRACK` of a sheet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueTrack {
    pub number: u32,
    /// The mode exactly as written, e.g. `MODE2/2352` or `AUDIO`.
    pub mode: String,
    /// `INDEX nn` positions in frames (sectors) from the start of the file.
    pub indexes: Vec<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueFile {
    /// The name exactly as written.
    pub name: String,
    pub file_type: String,
    pub tracks: Vec<CueTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CueSheet {
    pub files: Vec<CueFile>,
    /// `REM SESSION nn` lines seen.
    pub sessions: u32,
}

/// The data track YAOPLM turns into an ISO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTrack {
    pub file: String,
    pub mode: TrackMode,
    /// `INDEX 01`, in sectors from the start of the BIN.
    pub start_sector: u64,
    /// Sectors of the track when another track follows it in the same
    /// file; `None` means up to the end of the file.
    pub sectors: Option<u64>,
    /// Tracks after the data track, which the ISO leaves out.
    pub dropped: Vec<CueTrack>,
    /// The other files the sheet names (only dropped tracks are in them).
    pub other_files: Vec<String>,
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        // Older tools write the local code page; Latin-1 keeps every byte.
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

fn parse_msf(text: &str) -> Option<u32> {
    let mut parts = text.split(':').map(|p| p.parse::<u32>().ok());
    let (m, s, f) = (parts.next()??, parts.next()??, parts.next()??);
    if parts.next().is_some() || s >= 60 || f >= 75 {
        return None;
    }
    Some((m * 60 + s) * 75 + f).filter(|&frames| frames < MAX_FRAMES)
}

/// `FILE "name with spaces.bin" BINARY` or `FILE name.bin BINARY`.
fn parse_file_line(rest: &str) -> Option<(String, String)> {
    let rest = rest.trim();
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        let file_type = quoted[end + 1..].trim();
        return (!file_type.is_empty()).then(|| (quoted[..end].to_owned(), file_type.to_owned()));
    }
    let (name, file_type) = rest.rsplit_once(char::is_whitespace)?;
    let name = name.trim();
    (!name.is_empty()).then(|| (name.to_owned(), file_type.to_owned()))
}

impl CueSheet {
    /// Keywords are case-insensitive, lines may end in CR, LF or CRLF, a
    /// UTF-8 BOM is skipped. Metadata lines (`REM`, `TITLE`, `CATALOG`,
    /// `FLAGS`, ...) are ignored.
    pub fn parse(bytes: &[u8]) -> Result<Self, CueError> {
        let text = decode(bytes);
        let mut sheet = Self::default();
        for (index, raw) in text.split(['\n', '\r']).enumerate() {
            let line_no = index + 1;
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let (keyword, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
            let syntax = |problem: &str| CueError::Syntax {
                line: line_no,
                problem: problem.to_owned(),
            };
            match keyword.to_ascii_uppercase().as_str() {
                "FILE" => {
                    let (name, file_type) =
                        parse_file_line(rest).ok_or_else(|| syntax("bad FILE line"))?;
                    sheet.files.push(CueFile {
                        name,
                        file_type,
                        tracks: Vec::new(),
                    });
                }
                "TRACK" => {
                    let mut words = rest.split_whitespace();
                    let number = words
                        .next()
                        .and_then(|n| n.parse::<u32>().ok())
                        .ok_or_else(|| syntax("bad TRACK number"))?;
                    let mode = words.next().ok_or_else(|| syntax("TRACK without a mode"))?;
                    let file = sheet
                        .files
                        .last_mut()
                        .ok_or_else(|| syntax("TRACK before FILE"))?;
                    file.tracks.push(CueTrack {
                        number,
                        mode: mode.to_owned(),
                        indexes: Vec::new(),
                    });
                }
                "INDEX" => {
                    let mut words = rest.split_whitespace();
                    let number = words
                        .next()
                        .and_then(|n| n.parse::<u32>().ok())
                        .ok_or_else(|| syntax("bad INDEX number"))?;
                    let frames = words
                        .next()
                        .and_then(parse_msf)
                        .ok_or_else(|| syntax("bad INDEX position"))?;
                    let track = sheet
                        .files
                        .last_mut()
                        .and_then(|f| f.tracks.last_mut())
                        .ok_or_else(|| syntax("INDEX before TRACK"))?;
                    track.indexes.push((number, frames));
                }
                "REM" => {
                    let mut words = rest.split_whitespace();
                    if words
                        .next()
                        .is_some_and(|w| w.eq_ignore_ascii_case("SESSION"))
                    {
                        sheet.sessions += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(sheet)
    }

    /// Track 1 as the data track, the tracks dropped after it, or why this
    /// sheet cannot become an ISO.
    pub fn data_track(&self) -> Result<DataTrack, CueError> {
        if self.sessions > 1 {
            return Err(CueError::MultiSession);
        }
        let file = self.files.first().ok_or(CueError::NoFile)?;
        if !file.file_type.eq_ignore_ascii_case("BINARY") {
            return Err(CueError::FileType(file.file_type.clone()));
        }
        let track = file.tracks.first().ok_or(CueError::NoTrack)?;
        let mode =
            TrackMode::parse(&track.mode).ok_or_else(|| CueError::NotData(track.mode.clone()))?;
        let index = |t: &CueTrack, n: u32| {
            t.indexes
                .iter()
                .find(|(i, _)| *i == n)
                .map(|(_, frames)| u64::from(*frames))
        };
        let start = index(track, 1).ok_or(CueError::NoIndex)?;
        // In one file holding every track, the next one starts at its
        // pregap (INDEX 00) when the file has it, else at INDEX 01.
        let end = file
            .tracks
            .get(1)
            .and_then(|next| index(next, 0).or_else(|| index(next, 1)));
        let dropped: Vec<CueTrack> = self
            .files
            .iter()
            .flat_map(|f| &f.tracks)
            .skip(1)
            .cloned()
            .collect();
        if let Some(data) = dropped.iter().find(|t| TrackMode::parse(&t.mode).is_some()) {
            return Err(CueError::SecondDataTrack(data.number));
        }
        Ok(DataTrack {
            file: file.name.clone(),
            mode,
            start_sector: start,
            sectors: end.map(|end| end.saturating_sub(start)),
            dropped,
            other_files: self.files.iter().skip(1).map(|f| f.name.clone()).collect(),
        })
    }
}

/// Guesses the layout of a BIN without a sheet from its first sectors:
/// sector 16 must hold the primary volume descriptor (`\x01CD001`).
/// `head` should cover at least 17 raw sectors.
pub fn probe_raw(head: &[u8]) -> Option<TrackMode> {
    let pvd_at = |mode: TrackMode| -> bool {
        let start = 16 * mode.sector_size() as usize + mode.data_offset() as usize;
        head.get(start..start + 6) == Some(b"\x01CD001")
    };
    // Raw modes first: a 2048-byte ISO cannot have a PVD at the raw offsets.
    [
        TrackMode::Mode2_2352,
        TrackMode::Mode1_2352,
        TrackMode::Mode2_2336,
        TrackMode::Mode1_2048,
    ]
    .into_iter()
    .find(|&mode| pvd_at(mode))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BinError {
    #[error(
        "the track is {len} bytes from sector {start}, not a whole number of {sector_size}-byte sectors"
    )]
    PartialSector {
        len: u64,
        start: u64,
        sector_size: u32,
    },
    #[error("the BIN ends before INDEX 01")]
    StartPastEnd,
}

/// The user data of a BIN track as 2048-byte sectors.
#[derive(Debug)]
pub struct BinImage<R> {
    inner: R,
    mode: TrackMode,
    start: u64,
    sectors: u64,
    /// Byte position of `inner`, to skip seeks while reading in order.
    pos: Option<u64>,
    raw: Vec<u8>,
}

fn invalid(lba: u64, problem: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("BIN sector {lba}: {problem}"),
    )
}

impl<R: Read + Seek> BinImage<R> {
    /// `file_len` is the size of the BIN, `start_sector` the track's
    /// `INDEX 01`.
    pub fn new(
        inner: R,
        mode: TrackMode,
        start_sector: u64,
        file_len: u64,
    ) -> Result<Self, BinError> {
        let size = u64::from(mode.sector_size());
        let start = start_sector * size;
        let len = file_len.checked_sub(start).ok_or(BinError::StartPastEnd)?;
        if !len.is_multiple_of(size) {
            return Err(BinError::PartialSector {
                len,
                start: start_sector,
                sector_size: mode.sector_size(),
            });
        }
        Ok(Self::with_sectors(inner, mode, start_sector, len / size))
    }

    /// A track of `sectors` sectors from `start_sector`, for a BIN that
    /// holds more tracks after it.
    pub fn with_sectors(inner: R, mode: TrackMode, start_sector: u64, sectors: u64) -> Self {
        Self {
            inner,
            mode,
            start: start_sector * u64::from(mode.sector_size()),
            sectors,
            pos: None,
            raw: vec![0; mode.sector_size() as usize],
        }
    }

    pub fn mode(&self) -> TrackMode {
        self.mode
    }
}

/// The 2048 user-data bytes of a raw sector in `mode`, checked the way
/// research/09 §2 asks (sync, mode byte, subheader copies, no Form 2). An
/// empty Mode 0 sector reads as zeros. Also used for CHD frames.
pub fn user_data(mode: TrackMode, raw: &[u8], lba: u64, out: &mut Sector) -> io::Result<()> {
    if raw.len() < mode.sector_size() as usize {
        return Err(invalid(lba, "short sector"));
    }
    let data = match mode {
        TrackMode::Mode1_2048 => true,
        TrackMode::Mode2_2336 => {
            if raw[0..4] != raw[4..8] {
                return Err(invalid(lba, "subheader copies differ"));
            }
            if raw[2] & 0x20 != 0 {
                return Err(invalid(lba, "Mode 2 Form 2 sector"));
            }
            true
        }
        TrackMode::Mode1_2352 | TrackMode::Mode2_2352 => {
            if raw[..12] != SYNC {
                return Err(invalid(lba, "no sync pattern; wrong sector size?"));
            }
            match (raw[15], mode) {
                (0, _) if raw[16..2352].iter().all(|&b| b == 0) => false,
                (1, TrackMode::Mode1_2352) => true,
                (2, TrackMode::Mode2_2352) => {
                    if raw[16..20] != raw[20..24] {
                        return Err(invalid(lba, "subheader copies differ"));
                    }
                    if raw[18] & 0x20 != 0 {
                        return Err(invalid(lba, "Mode 2 Form 2 sector"));
                    }
                    true
                }
                (found, expected) => {
                    return Err(invalid(
                        lba,
                        &format!("sector is mode {found}, the sheet says {expected}"),
                    ));
                }
            }
        }
    };
    if data {
        let offset = mode.data_offset() as usize;
        out.copy_from_slice(&raw[offset..offset + SECTOR_SIZE]);
    } else {
        out.fill(0);
    }
    Ok(())
}

impl<R: Read + Seek> SectorSource for BinImage<R> {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        if lba >= self.sectors {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("BIN sector {lba} is past the end"),
            ));
        }
        let size = u64::from(self.mode.sector_size());
        let at = self.start + lba * size;
        if self.pos != Some(at) {
            self.inner.seek(SeekFrom::Start(at))?;
        }
        self.pos = None;
        self.inner.read_exact(&mut self.raw)?;
        self.pos = Some(at + size);
        user_data(self.mode, &self.raw, lba, buf)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn parses_common_sheets() {
        let sheet = CueSheet::parse(
            b"\xEF\xBB\xBFREM Generated\r\nfile \"Game (USA).bin\" binary\r\n  track 01 mode2/2352\r\n    INDEX 01 00:00:00\r\n",
        )
        .unwrap();
        let track = sheet.data_track().unwrap();
        assert_eq!(track.file, "Game (USA).bin");
        assert_eq!(track.mode, TrackMode::Mode2_2352);
        assert_eq!(track.start_sector, 0);
        assert_eq!(track.sectors, None);
        assert!(track.dropped.is_empty());

        let sheet = CueSheet::parse(
            b"CATALOG 0000000000000\rFILE game.bin BINARY\rTRACK 1 MODE1/2048\rPREGAP 00:02:00\rINDEX 00 00:00:00\rINDEX 01 00:02:00\r",
        )
        .unwrap();
        let track = sheet.data_track().unwrap();
        assert_eq!(track.file, "game.bin");
        assert_eq!(track.start_sector, 150);
    }

    #[test]
    fn multi_track_sheets_keep_the_data_track() {
        // Redump: one file per track.
        let sheet = CueSheet::parse(
            b"FILE \"Game (Track 1).bin\" BINARY\n  TRACK 01 MODE2/2352\n    INDEX 01 00:00:00\nFILE \"Game (Track 2).bin\" BINARY\n  TRACK 02 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:02:00\n",
        )
        .unwrap();
        let track = sheet.data_track().unwrap();
        assert_eq!(track.file, "Game (Track 1).bin");
        assert_eq!(track.sectors, None, "the data track is its whole file");
        assert_eq!(track.dropped.len(), 1);
        assert_eq!(track.dropped[0].mode, "AUDIO");
        assert_eq!(track.other_files, ["Game (Track 2).bin"]);

        // One file holding every track: the data ends at track 2's pregap.
        let sheet = CueSheet::parse(
            b"FILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 00 10:00:00\nINDEX 01 10:02:00\nTRACK 03 AUDIO\nINDEX 01 12:00:00\n",
        )
        .unwrap();
        let track = sheet.data_track().unwrap();
        assert_eq!(track.sectors, Some(10 * 60 * 75));
        assert_eq!(track.dropped.len(), 2);
        // Without INDEX 00 the next track starts at INDEX 01.
        let sheet = CueSheet::parse(
            b"FILE a.bin BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:00:30\n",
        )
        .unwrap();
        assert_eq!(sheet.data_track().unwrap().sectors, Some(30));
    }

    #[test]
    fn rejects_what_opl_cannot_play() {
        let reject = |text: &str| CueSheet::parse(text.as_bytes()).unwrap().data_track();
        assert_eq!(
            reject(
                "FILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:00\nTRACK 02 MODE2/2352\nINDEX 01 10:00:00\n"
            ),
            Err(CueError::SecondDataTrack(2))
        );
        assert_eq!(reject("REM nothing\n"), Err(CueError::NoFile));
        assert_eq!(reject("FILE a.bin BINARY\n"), Err(CueError::NoTrack));
        assert_eq!(
            reject("FILE a.wav WAVE\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n"),
            Err(CueError::FileType("WAVE".into()))
        );
        assert_eq!(
            reject("FILE a.bin BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n"),
            Err(CueError::NotData("AUDIO".into()))
        );
        assert_eq!(
            reject(
                "REM SESSION 01\nFILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:00:00\nREM SESSION 02\n"
            ),
            Err(CueError::MultiSession)
        );
        assert_eq!(
            reject("FILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 00 00:00:00\n"),
            Err(CueError::NoIndex)
        );
        assert!(CueSheet::parse(b"TRACK 01 MODE2/2352\n").is_err());
        assert!(
            CueSheet::parse(b"FILE a.bin BINARY\nTRACK 01 MODE2/2352\nINDEX 01 00:61:00\n")
                .is_err()
        );
    }

    fn raw_sector(mode: TrackMode, data: &[u8; 2048]) -> Vec<u8> {
        let mut raw = vec![0u8; mode.sector_size() as usize];
        match mode {
            TrackMode::Mode1_2048 => {}
            TrackMode::Mode2_2336 => raw[2] = 0x08,
            TrackMode::Mode1_2352 => {
                raw[..12].copy_from_slice(&SYNC);
                raw[15] = 1;
            }
            TrackMode::Mode2_2352 => {
                raw[..12].copy_from_slice(&SYNC);
                raw[15] = 2;
                raw[18] = 0x08;
                raw[22] = 0x08;
            }
        }
        if mode == TrackMode::Mode2_2336 {
            raw[6] = 0x08;
        }
        let at = mode.data_offset() as usize;
        raw[at..at + 2048].copy_from_slice(data);
        raw
    }

    #[test]
    fn extracts_user_data_in_every_mode() {
        for mode in [
            TrackMode::Mode1_2048,
            TrackMode::Mode1_2352,
            TrackMode::Mode2_2336,
            TrackMode::Mode2_2352,
        ] {
            let sectors: Vec<[u8; 2048]> = (0..20u8).map(|i| [i; 2048]).collect();
            let bin: Vec<u8> = sectors.iter().flat_map(|s| raw_sector(mode, s)).collect();
            let len = bin.len() as u64;
            let mut image = BinImage::new(Cursor::new(bin.clone()), mode, 0, len).unwrap();
            assert_eq!(image.sector_count(), 20);
            let mut buf = [0u8; 2048];
            image.read_sector(19, &mut buf).unwrap();
            assert_eq!(buf, [19u8; 2048], "{mode}");
            image.read_sector(3, &mut buf).unwrap();
            assert_eq!(buf, [3u8; 2048], "{mode}");
            assert!(image.read_sector(20, &mut buf).is_err());

            let mut head = bin.clone();
            let at = 16 * mode.sector_size() as usize + mode.data_offset() as usize;
            head[at..at + 6].copy_from_slice(b"\x01CD001");
            assert_eq!(probe_raw(&head), Some(mode));

            // Starting at INDEX 01 skips the pregap sectors.
            let mut image = BinImage::new(Cursor::new(bin), mode, 2, len).unwrap();
            assert_eq!(image.sector_count(), 18);
            image.read_sector(0, &mut buf).unwrap();
            assert_eq!(buf, [2u8; 2048]);
        }
        assert_eq!(probe_raw(&[0u8; 50_000]), None);
    }

    #[test]
    fn refuses_bad_sectors() {
        let data = [7u8; 2048];
        let good = raw_sector(TrackMode::Mode2_2352, &data);
        let mut form2 = good.clone();
        form2[18] |= 0x20;
        form2[22] |= 0x20;
        let mut no_sync = good.clone();
        no_sync[3] = 0;
        let mut mode1 = good.clone();
        mode1[15] = 1;
        for (bad, why) in [(form2, "Form 2"), (no_sync, "sync"), (mode1, "mode 1")] {
            let mut bin = good.clone();
            bin.extend_from_slice(&bad);
            let len = bin.len() as u64;
            let mut image = BinImage::new(Cursor::new(bin), TrackMode::Mode2_2352, 0, len).unwrap();
            let mut buf = [0u8; 2048];
            image.read_sector(0, &mut buf).unwrap();
            let err = image.read_sector(1, &mut buf).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::InvalidData, "{why}");
            assert!(err.to_string().contains(why), "{err}");
        }
        // Empty Mode 0 sectors read as zeros.
        let mut zero = vec![0u8; 2352];
        zero[..12].copy_from_slice(&SYNC);
        let len = zero.len() as u64;
        let mut image = BinImage::new(Cursor::new(zero), TrackMode::Mode2_2352, 0, len).unwrap();
        let mut buf = [1u8; 2048];
        image.read_sector(0, &mut buf).unwrap();
        assert_eq!(buf, [0u8; 2048]);

        assert_eq!(
            BinImage::new(Cursor::new(vec![0u8; 3000]), TrackMode::Mode2_2352, 0, 3000)
                .unwrap_err(),
            BinError::PartialSector {
                len: 3000,
                start: 0,
                sector_size: 2352
            }
        );
    }
}
