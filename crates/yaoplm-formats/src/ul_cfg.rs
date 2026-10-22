//! `ul.cfg` (USBExtreme/USBAdvance game table) and its 1 GiB chunk files
//! (IMP-5).
//!
//! Format reference: `docs/research/02-opl-formats.md` §5. Mirrors
//! `USBExtreme_game_entry_t` (`include/supportbase.h`), `sbReadList()`,
//! `sbRebuildULCfg()` and `sbCreatePath_name()` (`src/supportbase.c`) of
//! ps2homebrew/Open-PS2-Loader@b3a21997; the SMB part limit is `ISO_MAX_PARTS`
//! in `modules/iopcore/common/cdvd_config.h`, the 1 GiB step is
//! `DeviceReadSectors()` in `modules/iopcore/cdvdman/device-smb.c`.
//!
//! - The file is a bare array of 64-byte records. OPL reads `size / 64`
//!   records and never checks the magic or the 0x08 byte; a trailing partial
//!   record still produces one (garbage) entry.
//! - `name` (32 bytes) and `startup` (12 bytes) need no terminator.
//! - Chunks are `ul.%08X.%s.%02x`: [`usba_crc32`] of the title in uppercase
//!   hex, the game ID, the part number in lowercase hex. Every chunk except
//!   the last is exactly 1 GiB. iso2opl, OPL-PC-Tools and POUM print the part
//!   in decimal and PyOPLM/PS2IsoManager in uppercase hex; all agree for
//!   parts 00-09 only (research/REFERENCES.md §7).

use crate::media::Media;
use crate::usba_crc32::{is_portable, usba_crc32};

pub const RECORD_SIZE: usize = 64;
/// `UL_GAME_NAME_MAX`.
pub const NAME_MAX: usize = 32;
/// Width of the `startup` field (`GAME_STARTUP_MAX`).
pub const STARTUP_FIELD: usize = 12;
/// Game IDs from `SYSTEM.CNF` never exceed 11 characters.
pub const STARTUP_MAX: usize = 11;
/// Every chunk but the last: 0x80000 sectors of 2048 bytes.
pub const CHUNK_SIZE: u64 = 1 << 30;
/// `ISO_MAX_PARTS`: the in-game SMB driver keeps 10 file handles.
pub const SMB_MAX_PARTS: usize = 10;
pub const MAGIC: [u8; 3] = *b"ul.";
/// "to be compatible with original ul.cfg" (`sbRebuildULCfg`).
pub const BYTE_08: u8 = 0x08;

const SECTOR: u64 = 2048;
const OFF_MAGIC: usize = 0x20;
const OFF_STARTUP: usize = 0x23;
const OFF_PARTS: usize = 0x2F;
const OFF_MEDIA: usize = 0x30;
const OFF_BYTE08: usize = 0x35;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UlError {
    #[error("title is empty")]
    EmptyName,
    #[error("title has {0} bytes; ul.cfg holds {NAME_MAX}")]
    NameTooLong(usize),
    #[error("title must be printable 7-bit ASCII: OPL hashes other bytes unpredictably")]
    NameNotAscii,
    #[error("game ID {0:?} cannot be used in chunk file names")]
    BadStartup(String),
    #[error("a game needs 1 to 255 parts, not {0}")]
    BadPartCount(usize),
    #[error("image size must be a positive multiple of 2048 bytes, got {0}")]
    BadImageSize(u64),
}

/// Something in a record that OPL accepts but that is not what OPL writes,
/// or that breaks the game on the console.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordAnomaly {
    BadMagic,
    Byte08Missing,
    ReservedNotZero,
    /// OPL shows any media byte but 0x12 as DVD.
    UnknownMedia(u8),
    NoParts,
    EmptyName,
    /// Bytes >= 0x80: upstream OPL computes an unpredictable chunk hash.
    NonAsciiName,
    EmptyStartup,
}

/// One 64-byte `ul.cfg` record, kept byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UlRecord {
    bytes: [u8; RECORD_SIZE],
}

fn c_str(field: &[u8]) -> &[u8] {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    &field[..end]
}

fn is_valid_startup(startup: &str) -> bool {
    !startup.is_empty()
        && startup.len() <= STARTUP_MAX
        && startup.bytes().all(|b| {
            b.is_ascii_graphic()
                && !matches!(
                    b,
                    b'"' | b'*' | b'/' | b':' | b'<' | b'>' | b'?' | b'\\' | b'|'
                )
        })
}

impl UlRecord {
    /// A record as OPL writes it: zero-filled, magic, 0x08 at 0x35.
    pub fn new(name: &str, startup: &str, parts: usize, media: Media) -> Result<Self, UlError> {
        validate_name(name)?;
        if !is_valid_startup(startup) {
            return Err(UlError::BadStartup(startup.to_owned()));
        }
        let parts = u8::try_from(parts)
            .ok()
            .filter(|&p| p > 0)
            .ok_or(UlError::BadPartCount(parts))?;
        let mut bytes = [0u8; RECORD_SIZE];
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        bytes[OFF_MAGIC..OFF_MAGIC + 3].copy_from_slice(&MAGIC);
        bytes[OFF_STARTUP..OFF_STARTUP + startup.len()].copy_from_slice(startup.as_bytes());
        bytes[OFF_PARTS] = parts;
        bytes[OFF_MEDIA] = media.byte();
        bytes[OFF_BYTE08] = BYTE_08;
        Ok(Self { bytes })
    }

    pub fn from_bytes(bytes: [u8; RECORD_SIZE]) -> Self {
        Self { bytes }
    }

    pub fn as_bytes(&self) -> &[u8; RECORD_SIZE] {
        &self.bytes
    }

    /// The title up to its NUL, at most 32 bytes: what OPL displays and hashes.
    pub fn name_bytes(&self) -> &[u8] {
        c_str(&self.bytes[..NAME_MAX])
    }

    pub fn name(&self) -> String {
        String::from_utf8_lossy(self.name_bytes()).into_owned()
    }

    /// The game ID up to its NUL, at most 12 bytes.
    pub fn startup_bytes(&self) -> &[u8] {
        c_str(&self.bytes[OFF_STARTUP..OFF_STARTUP + STARTUP_FIELD])
    }

    pub fn startup(&self) -> String {
        String::from_utf8_lossy(self.startup_bytes()).into_owned()
    }

    pub fn parts(&self) -> u8 {
        self.bytes[OFF_PARTS]
    }

    pub fn media_byte(&self) -> u8 {
        self.bytes[OFF_MEDIA]
    }

    pub fn media(&self) -> Media {
        Media::from_byte_lossy(self.media_byte())
    }

    pub fn name_crc(&self) -> u32 {
        usba_crc32(self.name_bytes())
    }

    /// File name of chunk `part` in the device root.
    pub fn chunk_name(&self, part: u8) -> String {
        chunk_file_name(self.name_bytes(), self.startup_bytes(), part)
    }

    pub fn chunk_names(&self) -> Vec<String> {
        (0..self.parts())
            .map(|part| self.chunk_name(part))
            .collect()
    }

    /// The same game under a new title, as `sbRename()` writes it. The chunk
    /// files must be renamed too, since the hash changes.
    pub fn renamed(&self, name: &str) -> Result<Self, UlError> {
        validate_name(name)?;
        let mut bytes = self.bytes;
        bytes[..NAME_MAX].fill(0);
        bytes[..name.len()].copy_from_slice(name.as_bytes());
        Ok(Self { bytes })
    }

    pub fn anomalies(&self) -> Vec<RecordAnomaly> {
        let b = &self.bytes;
        let mut found = Vec::new();
        if b[OFF_MAGIC..OFF_MAGIC + 3] != MAGIC {
            found.push(RecordAnomaly::BadMagic);
        }
        if b[OFF_BYTE08] != BYTE_08 {
            found.push(RecordAnomaly::Byte08Missing);
        }
        if b[0x31..OFF_BYTE08]
            .iter()
            .chain(&b[0x36..])
            .any(|&x| x != 0)
        {
            found.push(RecordAnomaly::ReservedNotZero);
        }
        if Media::from_byte(self.media_byte()).is_none() {
            found.push(RecordAnomaly::UnknownMedia(self.media_byte()));
        }
        if self.parts() == 0 {
            found.push(RecordAnomaly::NoParts);
        }
        if self.name_bytes().is_empty() {
            found.push(RecordAnomaly::EmptyName);
        } else if !is_portable(self.name_bytes()) {
            found.push(RecordAnomaly::NonAsciiName);
        }
        if self.startup_bytes().is_empty() {
            found.push(RecordAnomaly::EmptyStartup);
        }
        found
    }
}

fn validate_name(name: &str) -> Result<(), UlError> {
    if name.is_empty() {
        return Err(UlError::EmptyName);
    }
    if !name.bytes().all(|b| (0x20..0x7F).contains(&b)) {
        return Err(UlError::NameNotAscii);
    }
    if name.len() > NAME_MAX {
        return Err(UlError::NameTooLong(name.len()));
    }
    Ok(())
}

/// `ul.%08X.%s.%02x`.
pub fn chunk_file_name(name: &[u8], startup: &[u8], part: u8) -> String {
    format!(
        "ul.{:08X}.{}.{:02x}",
        usba_crc32(name),
        String::from_utf8_lossy(startup),
        part
    )
}

/// A whole `ul.cfg` as OPL reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UlCfg {
    pub records: Vec<UlRecord>,
    /// Bytes after the last full record. OPL turns them into one more entry
    /// filled with leftovers of the previous record.
    pub trailing: Vec<u8>,
}

impl UlCfg {
    /// Never fails: like OPL, any byte string is some list of records.
    pub fn parse(bytes: &[u8]) -> Self {
        let (records, trailing) = bytes.as_chunks::<RECORD_SIZE>();
        Self {
            records: records.iter().copied().map(UlRecord::from_bytes).collect(),
            trailing: trailing.to_vec(),
        }
    }

    /// Full records only; a trailing fragment is dropped.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.records
            .iter()
            .flat_map(|record| record.as_bytes().iter().copied())
            .collect()
    }
}

/// One chunk of an image split for `ul`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkSpan {
    pub part: u8,
    pub offset: u64,
    pub len: u64,
}

/// Splits an image into 1 GiB chunks; only the last may be shorter.
pub fn split_plan(image_size: u64) -> Result<Vec<ChunkSpan>, UlError> {
    if image_size == 0 || !image_size.is_multiple_of(SECTOR) {
        return Err(UlError::BadImageSize(image_size));
    }
    let count = image_size.div_ceil(CHUNK_SIZE);
    let count = usize::try_from(count).unwrap_or(usize::MAX);
    if count > usize::from(u8::MAX) {
        return Err(UlError::BadPartCount(count));
    }
    Ok((0..count)
        .map(|i| {
            let offset = i as u64 * CHUNK_SIZE;
            ChunkSpan {
                part: i as u8,
                offset,
                len: (image_size - offset).min(CHUNK_SIZE),
            }
        })
        .collect())
}

/// True when the in-game SMB driver can open every part.
pub fn fits_smb(parts: usize) -> bool {
    parts <= SMB_MAX_PARTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_layout_matches_opl() {
        let r = UlRecord::new("Grand Theft Auto III", "SLUS_200.62", 2, Media::Dvd).unwrap();
        let b = r.as_bytes();
        assert_eq!(&b[..20], b"Grand Theft Auto III");
        assert!(b[20..32].iter().all(|&x| x == 0));
        assert_eq!(&b[0x20..0x23], b"ul.");
        assert_eq!(&b[0x23..0x2E], b"SLUS_200.62");
        assert_eq!(b[0x2E], 0);
        assert_eq!(b[0x2F], 2);
        assert_eq!(b[0x30], 0x14);
        assert_eq!(&b[0x31..0x35], &[0; 4]);
        assert_eq!(b[0x35], 0x08);
        assert_eq!(&b[0x36..], &[0; 10]);
        assert!(r.anomalies().is_empty());
    }

    #[test]
    fn full_width_name_has_no_terminator() {
        let name = "A".repeat(NAME_MAX);
        let r = UlRecord::new(&name, "SLUS_200.62", 1, Media::Cd).unwrap();
        assert_eq!(r.name_bytes(), name.as_bytes());
        assert_eq!(r.as_bytes()[0x20], b'u');
    }

    #[test]
    fn twelve_byte_startup_is_read_whole() {
        let mut bytes = *UlRecord::new("X", "SLUS_200.62", 1, Media::Cd)
            .unwrap()
            .as_bytes();
        bytes[0x2E] = b'9';
        assert_eq!(UlRecord::from_bytes(bytes).startup(), "SLUS_200.629");
    }

    #[test]
    fn validation() {
        assert_eq!(
            UlRecord::new("", "SLUS_200.62", 1, Media::Cd),
            Err(UlError::EmptyName)
        );
        assert_eq!(
            UlRecord::new(&"A".repeat(33), "SLUS_200.62", 1, Media::Cd),
            Err(UlError::NameTooLong(33))
        );
        assert_eq!(
            UlRecord::new("Ōkami", "SLUS_200.62", 1, Media::Cd),
            Err(UlError::NameNotAscii)
        );
        assert_eq!(
            UlRecord::new("Tab\there", "SLUS_200.62", 1, Media::Cd),
            Err(UlError::NameNotAscii)
        );
        assert!(matches!(
            UlRecord::new("X", "DATA\\MAIN.E", 1, Media::Cd),
            Err(UlError::BadStartup(_))
        ));
        assert!(matches!(
            UlRecord::new("X", "SLUS_200.629", 1, Media::Cd),
            Err(UlError::BadStartup(_))
        ));
        assert_eq!(
            UlRecord::new("X", "SLUS_200.62", 0, Media::Cd),
            Err(UlError::BadPartCount(0))
        );
        assert_eq!(
            UlRecord::new("X", "SLUS_200.62", 256, Media::Cd),
            Err(UlError::BadPartCount(256))
        );
    }

    #[test]
    fn chunk_names_use_upper_crc_and_lower_hex_part() {
        let r = UlRecord::new("Game", "SLUS_200.62", 11, Media::Dvd).unwrap();
        let crc = usba_crc32(b"Game");
        let names = r.chunk_names();
        assert_eq!(names.len(), 11);
        assert_eq!(names[0], format!("ul.{crc:08X}.SLUS_200.62.00"));
        assert_eq!(names[10], format!("ul.{crc:08X}.SLUS_200.62.0a"));
        // Zero-padded to 8 digits even for small hashes.
        assert_eq!(
            chunk_file_name(b"", b"ID", 0).len(),
            "ul.00000000.ID.00".len()
        );
    }

    #[test]
    fn rename_keeps_other_fields() {
        let r = UlRecord::new("Old Long Title", "SLUS_200.62", 3, Media::Dvd).unwrap();
        let renamed = r.renamed("New").unwrap();
        assert_eq!(renamed.name(), "New");
        assert_eq!(renamed.startup(), "SLUS_200.62");
        assert_eq!(renamed.parts(), 3);
        assert!(renamed.as_bytes()[3..32].iter().all(|&b| b == 0));
    }

    #[test]
    fn parse_and_serialize() {
        let a = UlRecord::new("A", "SLUS_000.01", 1, Media::Cd).unwrap();
        let b = UlRecord::new("B", "SLUS_000.02", 5, Media::Dvd).unwrap();
        let mut bytes = a.as_bytes().to_vec();
        bytes.extend_from_slice(b.as_bytes());
        let cfg = UlCfg::parse(&bytes);
        assert_eq!(cfg.records, vec![a, b]);
        assert!(cfg.trailing.is_empty());
        assert_eq!(cfg.to_bytes(), bytes);

        bytes.extend_from_slice(b"junk");
        let cfg = UlCfg::parse(&bytes);
        assert_eq!(cfg.records.len(), 2);
        assert_eq!(cfg.trailing, b"junk");

        assert_eq!(UlCfg::parse(&[]), UlCfg::default());
    }

    #[test]
    fn anomalies() {
        let mut bytes = [0u8; RECORD_SIZE];
        bytes[0] = 0xC5;
        bytes[0x30] = 0x01;
        bytes[0x40 - 1] = 1;
        let found = UlRecord::from_bytes(bytes).anomalies();
        assert_eq!(
            found,
            vec![
                RecordAnomaly::BadMagic,
                RecordAnomaly::Byte08Missing,
                RecordAnomaly::ReservedNotZero,
                RecordAnomaly::UnknownMedia(0x01),
                RecordAnomaly::NoParts,
                RecordAnomaly::NonAsciiName,
                RecordAnomaly::EmptyStartup,
            ]
        );
        assert_eq!(UlRecord::from_bytes(bytes).media(), Media::Dvd);
    }

    #[test]
    fn split_plan_uses_exact_gib_chunks() {
        let gib = CHUNK_SIZE;
        let plan = split_plan(4 * gib + 2048).unwrap();
        assert_eq!(plan.len(), 5);
        assert!(plan[..4].iter().all(|c| c.len == gib));
        assert_eq!(
            plan[4],
            ChunkSpan {
                part: 4,
                offset: 4 * gib,
                len: 2048
            }
        );
        assert_eq!(split_plan(gib).unwrap().len(), 1);
        assert_eq!(split_plan(gib + 2048).unwrap().len(), 2);
        // DVD-9 maximum (8.5 GB) needs 8 parts.
        assert_eq!(split_plan(4_173_824 * 2048).unwrap().len(), 8);
        assert_eq!(split_plan(0), Err(UlError::BadImageSize(0)));
        assert_eq!(split_plan(1000), Err(UlError::BadImageSize(1000)));
        assert_eq!(split_plan(256 * gib), Err(UlError::BadPartCount(256)));
        assert!(split_plan(255 * gib).is_ok());
    }

    #[test]
    fn smb_part_limit() {
        assert!(fits_smb(10));
        assert!(!fits_smb(11));
    }
}
