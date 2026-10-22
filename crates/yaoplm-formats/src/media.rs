//! Disc media type as OPL stores it.
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.4 and §5.1. Values
//! are `SCECdPS2CD` and `SCECdPS2DVD` from ps2sdk
//! `common/include/libcdvd-common.h`. OPL takes the type from the folder
//! (`CD/`, `DVD/`) or from the `ul.cfg` record, never from the image.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Media {
    Cd,
    Dvd,
}

impl Media {
    pub const CD_BYTE: u8 = 0x12;
    pub const DVD_BYTE: u8 = 0x14;

    pub const fn byte(self) -> u8 {
        match self {
            Self::Cd => Self::CD_BYTE,
            Self::Dvd => Self::DVD_BYTE,
        }
    }

    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            Self::CD_BYTE => Some(Self::Cd),
            Self::DVD_BYTE => Some(Self::Dvd),
            _ => None,
        }
    }

    /// How OPL reads an arbitrary media byte: anything but CD counts as DVD
    /// (`game->media == SCECdPS2CD ? "CD" : "DVD"` in `src/supportbase.c`).
    pub const fn from_byte_lossy(byte: u8) -> Self {
        if byte == Self::CD_BYTE {
            Self::Cd
        } else {
            Self::Dvd
        }
    }

    /// Image folder in the device root.
    pub const fn folder(self) -> &'static str {
        match self {
            Self::Cd => "CD",
            Self::Dvd => "DVD",
        }
    }
}

impl fmt::Display for Media {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.folder())
    }
}
