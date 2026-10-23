//! CHD images (IMP-1b): MAME's compressed hunks of data, as `chdman`
//! makes them from PS2 discs (`createdvd` for DVDs, `createcd` for CDs).
//!
//! OPL reads none of this (research/02 §4.2), so a CHD becomes an ISO on
//! import; this module gives its sectors to the same code that reads ISO,
//! ZSO and BIN. Layout, from MAME `src/lib/util/cdrom.cpp` and `chdcd.cpp`
//! as implemented by chd-rs 0.4:
//! - DVD: units are 2048-byte sectors and the logical data is the ISO.
//! - CD: units are 2448-byte frames, 2352 bytes of sector data and 96 of
//!   subcode. Tracks follow each other, each padded to a multiple of four
//!   frames, and `CHT2` (or older `CHTR`) metadata describes them:
//!   `TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:n PREGAP:p PGTYPE:t ...`.
//!   A track stores the first `datasize` bytes of each sector at the start
//!   of its frame: 2048 for `MODE1` and `MODE2_FORM1`, 2336 for `MODE2` and
//!   `MODE2_FORM_MIX`, 2352 for the `_RAW` types. A pregap whose type
//!   starts with `V` is stored in front of the track's data.
//!
//! Only track 1 can be a PS2 game; audio tracks after it are dropped like in
//! a multi-track BIN/CUE (`crate::cue`). CHDs that need a parent CHD are
//! refused.

use std::io::{self, Read, Seek, SeekFrom};

use chd::Chd;
use chd::metadata::{KnownMetadata, Metadata};
use chd::read::ChdReader;

use crate::cue::{RAW_SECTOR_SIZE, TrackMode, user_data};
use crate::iso9660::{SECTOR_SIZE, Sector, SectorSource};

/// Sector data plus subcode in a CD CHD frame.
pub const CD_FRAME: u64 = 2448;

#[derive(Debug, thiserror::Error)]
pub enum ChdError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("not a readable CHD: {0}")]
    Chd(String),
    #[error("this CHD needs a parent CHD; merge it first (chdman copy)")]
    NeedsParent,
    #[error("CD track metadata is unreadable: {0}")]
    Metadata(String),
    #[error("track 1 is {0}, not data")]
    NotData(String),
    #[error("track {0} is a second data track (mixed-mode or CD-Extra disc)")]
    SecondDataTrack(u32),
    #[error("a CHD of {unit}-byte units is neither a CD nor a DVD")]
    UnknownLayout { unit: u32 },
}

impl From<chd::Error> for ChdError {
    fn from(err: chd::Error) -> Self {
        match err {
            chd::Error::RequiresParent => Self::NeedsParent,
            other => Self::Chd(other.to_string()),
        }
    }
}

/// One track from `CHT2`/`CHTR` metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChdTrack {
    pub number: u32,
    /// `MODE2_RAW`, `MODE1`, `AUDIO`...
    pub kind: String,
    pub frames: u64,
    /// Frames of pregap stored in front of the data (`PGTYPE` starts with
    /// `V`); zero otherwise.
    pub stored_pregap: u64,
}

/// What the CHD holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// 2048-byte sectors.
    Dvd { sectors: u64 },
    /// The data track of a CD and what else is on the disc.
    Cd {
        mode: TrackMode,
        /// Frame of the track's first data sector.
        first_frame: u64,
        sectors: u64,
        /// Tracks after the data track, which the ISO leaves out.
        dropped: Vec<ChdTrack>,
    },
}

/// The bytes of the sector data a track stores per frame, and the mode
/// that reads them.
fn track_mode(kind: &str) -> Option<TrackMode> {
    Some(match kind {
        "MODE1" | "MODE2_FORM1" => TrackMode::Mode1_2048,
        "MODE2" | "MODE2_FORM_MIX" => TrackMode::Mode2_2336,
        "MODE1_RAW" => TrackMode::Mode1_2352,
        "MODE2_RAW" => TrackMode::Mode2_2352,
        _ => return None,
    })
}

/// Parses `TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:12345 PREGAP:150
/// PGTYPE:VMODE2_RAW ...` (a trailing NUL is fine).
pub fn parse_track(text: &str) -> Option<ChdTrack> {
    let field = |name: &str| {
        text.split(|c: char| c.is_whitespace() || c == '\0')
            .find_map(|part| part.strip_prefix(name)?.strip_prefix(':'))
    };
    let number = field("TRACK")?.parse().ok()?;
    let kind = field("TYPE")?.to_owned();
    let frames = field("FRAMES")?.parse().ok()?;
    let pregap: u64 = field("PREGAP").and_then(|p| p.parse().ok()).unwrap_or(0);
    let stored = field("PGTYPE").is_some_and(|t| t.starts_with('V'));
    Some(ChdTrack {
        number,
        kind,
        frames,
        stored_pregap: if stored { pregap } else { 0 },
    })
}

/// The CD tracks in a CHD's metadata, in order; empty for a DVD.
fn cd_tracks(metadata: &[Metadata]) -> Result<Vec<ChdTrack>, ChdError> {
    let mut tracks = Vec::new();
    for entry in metadata {
        let tag = entry.metatag;
        if tag == KnownMetadata::CdRomTrack2 as u32 || tag == KnownMetadata::CdRomTrack as u32 {
            let text = String::from_utf8_lossy(&entry.value);
            tracks.push(
                parse_track(&text)
                    .ok_or_else(|| ChdError::Metadata(text.trim_end_matches('\0').to_owned()))?,
            );
        } else if KnownMetadata::is_cdrom(tag) {
            return Err(ChdError::Metadata(
                "old CHD CD metadata (CHCD or GD-ROM); convert it with a recent chdman".into(),
            ));
        }
    }
    tracks.sort_by_key(|t| t.number);
    Ok(tracks)
}

/// A CHD read as 2048-byte ISO sectors.
pub struct ChdImage<R: Read + Seek> {
    reader: ChdReader<R>,
    layout: Layout,
    frame: Vec<u8>,
}

impl<R: Read + Seek> std::fmt::Debug for ChdImage<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChdImage")
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl<R: Read + Seek> ChdImage<R> {
    pub fn open(inner: R) -> Result<Self, ChdError> {
        let mut chd = Chd::open(inner, None)?;
        // chd-rs opens a child CHD without its parent and fails only when a
        // hunk refers to it, possibly halfway through a copy.
        if chd.header().has_parent() {
            return Err(ChdError::NeedsParent);
        }
        let metadata: Vec<Metadata> = chd.metadata_refs().try_into()?;
        let unit = chd.header().unit_bytes();
        let logical = chd.header().logical_bytes();
        let tracks = cd_tracks(&metadata)?;
        let layout = if tracks.is_empty() {
            if unit != SECTOR_SIZE as u32 {
                return Err(ChdError::UnknownLayout { unit });
            }
            Layout::Dvd {
                sectors: logical / SECTOR_SIZE as u64,
            }
        } else {
            let first = &tracks[0];
            let mode =
                track_mode(&first.kind).ok_or_else(|| ChdError::NotData(first.kind.clone()))?;
            if let Some(data) = tracks[1..].iter().find(|t| track_mode(&t.kind).is_some()) {
                return Err(ChdError::SecondDataTrack(data.number));
            }
            let sectors = first
                .frames
                .checked_sub(first.stored_pregap)
                .ok_or_else(|| {
                    ChdError::Metadata(format!(
                        "track 1 has {} frames but a pregap of {}",
                        first.frames, first.stored_pregap
                    ))
                })?;
            // The track must lie inside the CHD, or the copy fails at its end.
            if first
                .frames
                .checked_mul(CD_FRAME)
                .is_none_or(|end| end > logical)
            {
                return Err(ChdError::Metadata(format!(
                    "track 1 ({} frames) runs past the end of the CHD",
                    first.frames
                )));
            }
            Layout::Cd {
                mode,
                first_frame: first.stored_pregap,
                sectors,
                dropped: tracks[1..].to_vec(),
            }
        };
        Ok(Self {
            reader: ChdReader::new(chd),
            layout,
            frame: vec![0; CD_FRAME as usize],
        })
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }
}

impl<R: Read + Seek> SectorSource for ChdImage<R> {
    fn sector_count(&self) -> u64 {
        match &self.layout {
            Layout::Dvd { sectors } | Layout::Cd { sectors, .. } => *sectors,
        }
    }

    fn read_sector(&mut self, lba: u64, buf: &mut Sector) -> io::Result<()> {
        if lba >= self.sector_count() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("CHD sector {lba} is past the end"),
            ));
        }
        match &self.layout {
            Layout::Dvd { .. } => {
                self.reader
                    .seek(SeekFrom::Start(lba * SECTOR_SIZE as u64))?;
                self.reader.read_exact(buf)
            }
            Layout::Cd {
                mode, first_frame, ..
            } => {
                let mode = *mode;
                self.reader
                    .seek(SeekFrom::Start((first_frame + lba) * CD_FRAME))?;
                let data = &mut self.frame[..mode.sector_size() as usize];
                self.reader.read_exact(data)?;
                user_data(mode, data, lba, buf)
            }
        }
    }

    fn raw_sector(&self) -> Option<&[u8]> {
        match &self.layout {
            Layout::Cd { mode, .. } if mode.sector_size() == RAW_SECTOR_SIZE => {
                Some(&self.frame[..RAW_SECTOR_SIZE as usize])
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_metadata() {
        let track = parse_track(
            "TRACK:1 TYPE:MODE2_RAW SUBTYPE:NONE FRAMES:9000 PREGAP:150 PGTYPE:VMODE2_RAW PGSUB:RW POSTGAP:0\0",
        )
        .unwrap();
        assert_eq!(track.number, 1);
        assert_eq!(track.kind, "MODE2_RAW");
        assert_eq!(track.frames, 9000);
        assert_eq!(track.stored_pregap, 150);
        let old = parse_track("TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:300").unwrap();
        assert_eq!(old.stored_pregap, 0);
        assert_eq!(
            parse_track("TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:10 PREGAP:150 PGTYPE:MODE1")
                .unwrap()
                .stored_pregap,
            0
        );
        assert!(parse_track("garbage").is_none());
        assert_eq!(track_mode("MODE2_FORM1"), Some(TrackMode::Mode1_2048));
        assert_eq!(track_mode("MODE2_FORM2"), None);
        assert_eq!(track_mode("AUDIO"), None);
    }
}
