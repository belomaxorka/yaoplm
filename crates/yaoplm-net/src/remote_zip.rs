//! The parts of a zip archive needed to read single members over HTTP
//! range requests (ADR-0035, research/14 §3): the end records, the central
//! directory and a member's local header (PKWARE APPNOTE 6.3.10 §4.3).
//!
//! The OPL Manager backups on archive.org are 5-6 GB zip64 files whose
//! members are stored without compression, and their local headers carry
//! data descriptors with zeroed sizes. So sizes, CRCs and offsets always
//! come from the central directory, never from a local header.

use crate::NetError;

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;

const EOCD_LEN: usize = 22;
const ZIP64_LOCATOR_LEN: usize = 20;
pub const ZIP64_EOCD_LEN: usize = 56;
const CENTRAL_LEN: usize = 46;
pub const LOCAL_LEN: usize = 30;
/// The end records with the longest comment a zip can have: enough of the
/// tail to find the central directory in one request.
pub const TAIL_LEN: u64 =
    (EOCD_LEN + u16::MAX as usize + ZIP64_LOCATOR_LEN + ZIP64_EOCD_LEN) as u64;

/// Where the central directory is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Directory {
    pub offset: u64,
    pub size: u64,
    pub entries: u64,
}

/// What the tail of an archive says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tail {
    Directory(Directory),
    /// A zip64 archive whose zip64 end record is not in the tail that was
    /// read: read [`ZIP64_EOCD_LEN`] bytes at this offset next.
    Zip64RecordAt(u64),
}

/// One member, as the central directory describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub method: u16,
    pub crc32: u32,
    pub compressed_size: u64,
    pub size: u64,
    /// Offset of the local header.
    pub offset: u64,
}

fn bad(what: impl Into<String>) -> NetError {
    NetError::Zip(what.into())
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

/// Finds the central directory from the last bytes of an archive; `tail`
/// starts at `tail_start` in the file.
pub fn parse_tail(tail: &[u8], tail_start: u64) -> Result<Tail, NetError> {
    // The last end record whose comment reaches exactly to the end.
    let eocd = (0..=tail.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&at| {
            u32_at(tail, at) == Some(EOCD_SIG)
                && u16_at(tail, at + 20)
                    .is_some_and(|comment| at + EOCD_LEN + usize::from(comment) == tail.len())
        })
        .ok_or_else(|| bad("no end of central directory record"))?;
    let read = |at: usize, f: fn(&[u8], usize) -> Option<u64>| {
        f(tail, eocd + at).ok_or_else(|| bad("short end record"))
    };
    let entries = read(10, |b, a| u16_at(b, a).map(u64::from))?;
    let size = read(12, |b, a| u32_at(b, a).map(u64::from))?;
    let offset = read(16, |b, a| u32_at(b, a).map(u64::from))?;
    let zip64 = entries == 0xFFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF;
    let locator = eocd.checked_sub(ZIP64_LOCATOR_LEN);
    match locator.filter(|&at| u32_at(tail, at) == Some(ZIP64_LOCATOR_SIG)) {
        Some(at) => {
            let record = u64_at(tail, at + 8).ok_or_else(|| bad("short zip64 locator"))?;
            match record
                .checked_sub(tail_start)
                .and_then(|rel| usize::try_from(rel).ok())
                .filter(|&rel| rel + ZIP64_EOCD_LEN <= tail.len())
            {
                Some(rel) => {
                    parse_zip64_record(&tail[rel..rel + ZIP64_EOCD_LEN]).map(Tail::Directory)
                }
                None => Ok(Tail::Zip64RecordAt(record)),
            }
        }
        None if zip64 => Err(bad("zip64 values without a zip64 locator")),
        None => Ok(Tail::Directory(Directory {
            offset,
            size,
            entries,
        })),
    }
}

/// The zip64 end of central directory record.
pub fn parse_zip64_record(bytes: &[u8]) -> Result<Directory, NetError> {
    if u32_at(bytes, 0) != Some(ZIP64_EOCD_SIG) {
        return Err(bad("no zip64 end of central directory record"));
    }
    let field = |at| u64_at(bytes, at).ok_or_else(|| bad("short zip64 end record"));
    Ok(Directory {
        entries: field(32)?,
        size: field(40)?,
        offset: field(48)?,
    })
}

/// Every member of a central directory, with zip64 sizes and offsets
/// taken from the extra field where the header holds `0xFFFFFFFF`.
pub fn parse_central_directory(bytes: &[u8]) -> Result<Vec<Member>, NetError> {
    let mut members = Vec::new();
    let mut at = 0usize;
    while at < bytes.len() {
        let header = bytes
            .get(at..at + CENTRAL_LEN)
            .ok_or_else(|| bad(format!("central directory cut short at {at}")))?;
        if u32_at(header, 0) != Some(CENTRAL_SIG) {
            return Err(bad(format!("bad central directory header at {at}")));
        }
        let field16 = |o| u16_at(header, o).map(usize::from).unwrap_or_default();
        let field32 = |o| u32_at(header, o).unwrap_or_default();
        let (name_len, extra_len, comment_len) = (field16(28), field16(30), field16(32));
        let name_start = at + CENTRAL_LEN;
        let extra_start = name_start + name_len;
        let next = extra_start + extra_len + comment_len;
        let name = bytes
            .get(name_start..extra_start)
            .ok_or_else(|| bad(format!("central directory name cut short at {at}")))?;
        let extra = bytes
            .get(extra_start..extra_start + extra_len)
            .ok_or_else(|| bad(format!("central directory extra field cut short at {at}")))?;
        let mut size = u64::from(field32(24));
        let mut compressed_size = u64::from(field32(20));
        let mut offset = u64::from(field32(42));
        if [size, compressed_size, offset].contains(&0xFFFF_FFFF) {
            let mut zip64 = zip64_extra(extra)
                .ok_or_else(|| bad(format!("zip64 sizes without a zip64 field at {at}")))?
                .as_chunks::<8>()
                .0
                .iter()
                .map(|c| u64::from_le_bytes(*c));
            // Only the fields that overflowed are there, in this order.
            for value in [&mut size, &mut compressed_size, &mut offset] {
                if *value == 0xFFFF_FFFF {
                    *value = zip64
                        .next()
                        .ok_or_else(|| bad(format!("short zip64 field at {at}")))?;
                }
            }
        }
        members.push(Member {
            name: String::from_utf8_lossy(name).into_owned(),
            method: u16_at(header, 10).unwrap_or_default(),
            crc32: field32(16),
            compressed_size,
            size,
            offset,
        });
        at = next;
    }
    Ok(members)
}

/// The data of the zip64 extended information field (header id 1).
fn zip64_extra(mut extra: &[u8]) -> Option<&[u8]> {
    while extra.len() >= 4 {
        let id = u16_at(extra, 0)?;
        let len = usize::from(u16_at(extra, 2)?);
        let data = extra.get(4..4 + len)?;
        if id == 1 {
            return Some(data);
        }
        extra = &extra[4 + len..];
    }
    None
}

/// Where a member's data starts, counted from its local header, which
/// `header` begins with.
pub fn data_start(header: &[u8]) -> Result<u64, NetError> {
    if u32_at(header, 0) != Some(LOCAL_SIG) {
        return Err(bad("no local file header at the member's offset"));
    }
    let name = u16_at(header, 26).ok_or_else(|| bad("short local header"))?;
    let extra = u16_at(header, 28).ok_or_else(|| bad("short local header"))?;
    Ok(LOCAL_LEN as u64 + u64::from(name) + u64::from(extra))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::{Cursor, Write};

    use zip::write::SimpleFileOptions;

    use super::*;

    /// A stored zip; with `zip64`, every member gets zip64 fields.
    pub(crate) fn build_zip(members: &[(&str, Vec<u8>)], zip64: bool) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(zip64);
        for (name, bytes) in members {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.set_comment("a comment").unwrap();
        let zip = writer.finish().unwrap().into_inner();
        if zip64 { with_zip64_end(&zip) } else { zip }
    }

    /// What a zip64 writer leaves when the directory lies past 4 GiB: the
    /// old end record full of `0xFFFF`, a zip64 end record and a locator,
    /// and central headers whose sizes and offsets are in zip64 fields.
    fn with_zip64_end(zip: &[u8]) -> Vec<u8> {
        let eocd = (0..zip.len() - EOCD_LEN)
            .rev()
            .find(|&at| u32_at(zip, at) == Some(EOCD_SIG))
            .unwrap();
        let entries = u64::from(u16_at(zip, eocd + 10).unwrap());
        let offset = u32_at(zip, eocd + 16).unwrap() as usize;
        let comment = &zip[eocd + EOCD_LEN..];
        let mut out = zip[..offset].to_vec();
        // Central headers again, with every size and offset moved out.
        let old = parse_central_directory(&zip[offset..eocd]).unwrap();
        let mut at = offset;
        for member in &old {
            let name_len = usize::from(u16_at(zip, at + 28).unwrap());
            let extra_len = usize::from(u16_at(zip, at + 30).unwrap());
            let mut header = zip[at..at + CENTRAL_LEN].to_vec();
            header[20..28].copy_from_slice(&[0xFF; 8]);
            header[42..46].copy_from_slice(&[0xFF; 4]);
            // An unrelated field first, then the zip64 one.
            let mut extra = vec![0x55, 0x54, 1, 0, 0];
            extra.extend_from_slice(&1u16.to_le_bytes());
            extra.extend_from_slice(&24u16.to_le_bytes());
            for value in [member.size, member.compressed_size, member.offset] {
                extra.extend_from_slice(&value.to_le_bytes());
            }
            header[30..32].copy_from_slice(&(extra.len() as u16).to_le_bytes());
            header[32..34].copy_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&header);
            out.extend_from_slice(&zip[at + CENTRAL_LEN..at + CENTRAL_LEN + name_len]);
            out.extend_from_slice(&extra);
            at += CENTRAL_LEN + name_len + extra_len + usize::from(u16_at(zip, at + 32).unwrap());
        }
        let size = (out.len() - offset) as u64;
        let record = out.len() as u64;
        out.extend_from_slice(&ZIP64_EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&44u64.to_le_bytes());
        out.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for value in [entries, entries, size, offset as u64] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&ZIP64_LOCATOR_SIG.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&record.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
        out.extend_from_slice(&[0xFF; 8]);
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    fn members_of(zip: &[u8], tail_len: usize) -> Vec<Member> {
        let start = zip.len().saturating_sub(tail_len);
        let directory = match parse_tail(&zip[start..], start as u64).unwrap() {
            Tail::Directory(d) => d,
            Tail::Zip64RecordAt(at) => {
                let at = at as usize;
                parse_zip64_record(&zip[at..at + ZIP64_EOCD_LEN]).unwrap()
            }
        };
        let (from, to) = (
            directory.offset as usize,
            (directory.offset + directory.size) as usize,
        );
        let members = parse_central_directory(&zip[from..to]).unwrap();
        assert_eq!(members.len() as u64, directory.entries);
        members
    }

    #[test]
    fn reads_members_of_plain_and_zip64_archives() {
        let pictures = [
            ("PS2/SLUS_200.71/SLUS_200.71_COV.png", vec![1u8; 300]),
            ("PS2/SLUS_200.71/SLUS_200.71_ICO.png", vec![2u8; 40]),
        ];
        for zip64 in [false, true] {
            let zip = build_zip(&pictures, zip64);
            let tail_start = zip.len() - 22 - "a comment".len() - 20;
            assert_eq!(
                matches!(
                    parse_tail(&zip[tail_start..], tail_start as u64),
                    Ok(Tail::Zip64RecordAt(_))
                ),
                zip64,
                "a short tail leaves the zip64 record to read next"
            );
            // The whole tail, and one too short to hold the zip64 record.
            for tail in [TAIL_LEN as usize, 60] {
                let members = members_of(&zip, tail);
                assert_eq!(members.len(), 2);
                for (member, (name, bytes)) in members.iter().zip(&pictures) {
                    assert_eq!(member.name, *name);
                    assert_eq!(member.method, 0);
                    assert_eq!(member.size, bytes.len() as u64);
                    assert_eq!(member.compressed_size, bytes.len() as u64);
                    assert_eq!(member.crc32, crc32fast::hash(bytes));
                    let local = &zip[member.offset as usize..];
                    let start = member.offset as usize + data_start(local).unwrap() as usize;
                    assert_eq!(&zip[start..start + bytes.len()], bytes.as_slice());
                }
            }
        }
    }

    #[test]
    fn damaged_archives_are_errors() {
        let zip = build_zip(&[("a.png", vec![0; 10])], false);
        assert!(parse_tail(&zip[..zip.len() - 3], 0).is_err());
        assert!(parse_tail(b"not a zip at all, no records here", 0).is_err());
        let members = members_of(&zip, zip.len());
        let directory = &zip[members[0].offset as usize..];
        assert!(parse_central_directory(directory).is_err());
        assert!(data_start(b"PK\x01\x02 not a local header......").is_err());
        let mut cut = zip.clone();
        let eocd = zip.len() - 22 - "a comment".len();
        let cd_start = u32::from_le_bytes(zip[eocd + 16..eocd + 20].try_into().unwrap()) as usize;
        cut.truncate(eocd - 5);
        assert!(parse_central_directory(&cut[cd_start..]).is_err());
    }
}
