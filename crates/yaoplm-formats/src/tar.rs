//! Reading tar archives: RiptOPL's and wOPL's `ART/art.tar` picture sets
//! (ART-5).
//!
//! Format reference: `docs/research/02-opl-formats.md` §7 and research/07
//! §4.4 (RiptOPL `include/tar.h`, `src/tar.c`; wOPL `src/tar.c`). Both read
//! POSIX ustar in 512-byte blocks and index regular files only. RiptOPL
//! skips zero blocks instead of stopping at the first end marker, so packs
//! joined with `cat` work there; we read the same way. GNU long names
//! (`L`) and the `path` of pax headers (`x`) name the next file; other
//! entry types are skipped. Only the logic is followed, no code is copied
//! (the loaders are AFL, ADR-0002).

use std::io::{self, Read};

const BLOCK: usize = 512;
/// A long name larger than this is not a file name.
const MAX_LONG_NAME: u64 = 64 << 10;

#[derive(Debug, thiserror::Error)]
pub enum TarError {
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The block at `offset` is not a tar header (its checksum is wrong).
    #[error("not a tar archive: no valid header at byte {offset}")]
    BadHeader { offset: u64 },
}

/// A regular file in a tar archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TarEntry {
    /// The path inside the archive, with `/` separators.
    pub name: String,
    pub size: u64,
}

/// `field` as a NUL- or space-terminated string.
fn text(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A numeric field: octal digits, or base-256 when the top bit is set
/// (GNU, for sizes over 8 GiB).
fn number(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        let mut value: u64 = u64::from(field[0] & 0x7F);
        for &b in &field[1..] {
            value = value.checked_mul(256)?.checked_add(u64::from(b))?;
        }
        return Some(value);
    }
    let digits: String = field
        .iter()
        .map(|&b| b as char)
        .skip_while(|c| *c == ' ')
        .take_while(|c| c.is_digit(8))
        .collect();
    if digits.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(&digits, 8).ok()
}

/// The header's checksum as the sum of its bytes, unsigned or, as old tars
/// wrote it, signed (GNU tar and Python's `tarfile` accept both): the two
/// differ once a name has bytes over 0x7F, a Cyrillic folder for one.
fn checksum_ok(header: &[u8; BLOCK]) -> bool {
    let Some(stored) = number(&header[148..156]) else {
        return false;
    };
    let (mut unsigned, mut signed) = (0u64, 0i64);
    for (i, &b) in header.iter().enumerate() {
        let b = if (148..156).contains(&i) { b' ' } else { b };
        unsigned += u64::from(b);
        signed += i64::from(b.cast_signed());
    }
    stored == unsigned || i64::try_from(stored).is_ok_and(|s| s == signed)
}

/// Reads exactly `buf.len()` bytes, or none at the end of the stream.
fn read_block(reader: &mut dyn Read, buf: &mut [u8; BLOCK]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < BLOCK {
        match reader.read(&mut buf[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the archive ends inside a block",
                ));
            }
            Ok(n) => filled += n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(true)
}

/// The `path` record of a pax extended header, if it has one.
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    while !rest.is_empty() {
        let space = rest.iter().position(|&b| b == b' ')?;
        let len: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        let record = rest.get(space + 1..len)?;
        if let Some(value) = record.strip_prefix(b"path=") {
            let value = value.strip_suffix(b"\n").unwrap_or(value);
            return Some(String::from_utf8_lossy(value).into_owned());
        }
        rest = rest.get(len..)?;
    }
    None
}

fn ends_inside(what: &str) -> TarError {
    TarError::Io(io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("the archive ends inside {what}"),
    ))
}

/// Hands every regular file of the archive to `visit`, in order, with a
/// reader of its bytes; what `visit` leaves unread is skipped. An archive
/// that ends inside the data of an entry, a file or one that is skipped,
/// is an [`io::ErrorKind::UnexpectedEof`] error.
pub fn for_each_file<R: Read>(
    mut reader: R,
    visit: &mut dyn FnMut(&TarEntry, &mut dyn Read) -> io::Result<()>,
) -> Result<(), TarError> {
    let mut header = [0u8; BLOCK];
    let mut offset = 0u64;
    let mut long_name: Option<String> = None;
    loop {
        if !read_block(&mut reader, &mut header)? {
            return Ok(());
        }
        let at = offset;
        offset += BLOCK as u64;
        if header.iter().all(|&b| b == 0) {
            continue;
        }
        if !checksum_ok(&header) {
            return Err(TarError::BadHeader { offset: at });
        }
        // A base-256 size near 2^64 has no padded length.
        let (size, padded) = number(&header[124..136])
            .and_then(|size| Some((size, size.checked_next_multiple_of(BLOCK as u64)?)))
            .ok_or(TarError::BadHeader { offset: at })?;
        let kind = header[156];
        match kind {
            b'L' | b'x' if size <= MAX_LONG_NAME => {
                let mut data = vec![0u8; usize::try_from(size).unwrap_or(0)];
                reader.read_exact(&mut data)?;
                io::copy(&mut (&mut reader).take(padded - size), &mut io::sink())?;
                offset += padded;
                long_name = if kind == b'L' {
                    Some(text(&data))
                } else {
                    pax_path(&data).or(long_name)
                };
                continue;
            }
            b'0' | 0 => {}
            _ => {
                // Copied, not seeked: the header may claim more than the
                // archive holds. A cut in the padding is let go, as after
                // a file.
                let skipped = io::copy(&mut (&mut reader).take(padded), &mut io::sink())?;
                if skipped < size {
                    return Err(ends_inside("an entry"));
                }
                offset += skipped;
                long_name = None;
                continue;
            }
        }
        let name = match long_name.take() {
            Some(name) => name,
            None => {
                let name = text(&header[0..100]);
                let prefix = text(&header[345..500]);
                if &header[257..262] == b"ustar" && !prefix.is_empty() {
                    format!("{prefix}/{name}")
                } else {
                    name
                }
            }
        };
        let entry = TarEntry { name, size };
        let mut data = (&mut reader).take(size);
        visit(&entry, &mut data)?;
        io::copy(&mut data, &mut io::sink())?;
        if data.limit() > 0 {
            return Err(ends_inside("a file"));
        }
        io::copy(&mut (&mut reader).take(padded - size), &mut io::sink())?;
        offset += padded;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ustar header for `name` of `size` bytes with the right checksum.
    fn header(name: &str, size: u64, kind: u8) -> Vec<u8> {
        let mut h = vec![0u8; BLOCK];
        let (prefix, name) = match name.rsplit_once('/') {
            Some((p, n)) if name.len() > 100 => (p, n),
            _ => ("", name),
        };
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        seal(&mut h);
        h
    }

    /// Writes the unsigned checksum of `h`.
    fn seal(h: &mut [u8]) {
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    }

    fn file(name: &str, data: &[u8]) -> Vec<u8> {
        let mut out = header(name, data.len() as u64, b'0');
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
        out
    }

    fn read_all(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, TarError> {
        let mut out = Vec::new();
        for_each_file(bytes, &mut |entry, data| {
            let mut buf = Vec::new();
            data.read_to_end(&mut buf)?;
            out.push((entry.name.clone(), buf));
            Ok(())
        })?;
        Ok(out)
    }

    #[test]
    fn files_in_order_with_joined_archives_and_long_names() {
        let mut tar = file("ART/SLUS_200.62_COV.png", b"cover");
        tar.extend(header("ART", 0, b'5'));
        tar.extend(vec![0u8; BLOCK * 2]);
        // A second archive joined with `cat`.
        tar.extend(file("SLUS_200.62_ICO.png", &[7u8; 700]));
        let long = format!("{}/SLES_123.45_BG.png", "deep/".repeat(30));
        let mut l = header("././@LongLink", long.len() as u64 + 1, b'L');
        l.extend_from_slice(long.as_bytes());
        l.push(0);
        l.resize(l.len().div_ceil(BLOCK) * BLOCK, 0);
        tar.extend(l);
        tar.extend(file("ignored-name", b"bg"));
        tar.extend(vec![0u8; BLOCK * 2]);
        let files = read_all(&tar).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "ART/SLUS_200.62_COV.png",
                "SLUS_200.62_ICO.png",
                long.as_str()
            ]
        );
        assert_eq!(files[0].1, b"cover");
        assert_eq!(files[1].1.len(), 700);
        assert_eq!(files[2].1, b"bg");
    }

    #[test]
    fn unread_data_is_skipped_and_damage_is_reported() {
        let mut tar = file("a.png", &[1u8; 1500]);
        tar.extend(file("b.png", b"second"));
        let mut names = Vec::new();
        for_each_file(&tar[..], &mut |entry, _| {
            names.push(entry.name.clone());
            Ok(())
        })
        .unwrap();
        assert_eq!(names, ["a.png", "b.png"]);

        let mut broken = tar.clone();
        broken[3] ^= 1;
        assert!(matches!(
            read_all(&broken),
            Err(TarError::BadHeader { offset: 0 })
        ));
        assert!(read_all(b"not a tar file at all, just text").is_err());
        assert!(read_all(&tar[..1000]).is_err(), "cut inside a file");
        assert_eq!(read_all(&[]).unwrap(), []);
    }

    #[test]
    fn numbers_and_pax_paths() {
        assert_eq!(number(b"00000001750\0"), Some(1000));
        assert_eq!(number(b"     144 \0"), Some(100));
        assert_eq!(
            number(&[0x80, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0]),
            Some(1 << 33)
        );
        assert_eq!(
            pax_path(b"32 path=ART/SLUS_200.62_COV.png\n"),
            Some("ART/SLUS_200.62_COV.png".into())
        );
        assert_eq!(pax_path(b"20 mtime=1234567890\n"), None);
    }

    #[test]
    fn a_size_near_the_u64_limit_is_a_bad_header() {
        // A file, then a folder.
        for &kind in b"05" {
            let mut h = header("a.png", 0, kind);
            // Base-256: u64::MAX - 100, which rounds up past 2^64.
            h[124..136].copy_from_slice(&[
                0x80, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x9B,
            ]);
            seal(&mut h);
            assert_eq!(number(&h[124..136]), Some(u64::MAX - 100));
            assert!(
                matches!(read_all(&h), Err(TarError::BadHeader { offset: 0 })),
                "kind {kind}"
            );
        }
        // A skipped entry of 2^64 - 512 bytes, a whole number of blocks,
        // has a padded length; the archive holds far less.
        let mut h = header("dir", 0, b'5');
        h[124..136].copy_from_slice(&[0x80, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0]);
        seal(&mut h);
        h.extend(vec![0u8; BLOCK]);
        assert!(is_cut(read_all(&h)));
    }

    fn is_cut(result: Result<Vec<(String, Vec<u8>)>, TarError>) -> bool {
        matches!(result, Err(TarError::Io(e)) if e.kind() == io::ErrorKind::UnexpectedEof)
    }

    #[test]
    fn a_skipped_entry_cut_short_is_an_error() {
        // A folder that claims 1024 bytes of data, then 100 of them.
        let mut tar = header("ART", 1024, b'5');
        tar.extend(vec![7u8; 100]);
        assert!(is_cut(read_all(&tar)));
        // Whole, it is passed over.
        tar.resize(BLOCK + 1024, 7);
        tar.extend(file("SLUS_200.62_COV.png", b"cover"));
        assert_eq!(
            read_all(&tar).unwrap(),
            [("SLUS_200.62_COV.png".to_owned(), b"cover".to_vec())]
        );
        // So is one cut in its padding, as a file is.
        let mut tar = header("ART", 1000, b'5');
        tar.extend(vec![7u8; 1000]);
        assert_eq!(read_all(&tar).unwrap(), []);
        let mut tar = file("a.png", &[1u8; 1000]);
        tar.truncate(BLOCK + 1000);
        assert_eq!(read_all(&tar).unwrap().len(), 1);
    }

    #[test]
    fn signed_checksums_are_accepted() {
        let name = "ART/Обложки/SLUS_200.62_COV.png";
        let mut tar = file(name, b"cover");
        let signed: i64 = tar[..BLOCK]
            .iter()
            .enumerate()
            .map(|(i, &b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    i64::from(b.cast_signed())
                }
            })
            .sum();
        assert!(signed > 0, "fits the octal field");
        tar[148..156].copy_from_slice(format!("{signed:06o}\0 ").as_bytes());
        let files = read_all(&tar).unwrap();
        assert_eq!(files, [(name.to_owned(), b"cover".to_vec())]);

        // Neither sum: still not a header.
        tar[148..156].copy_from_slice(format!("{:06o}\0 ", signed + 1).as_bytes());
        assert!(matches!(
            read_all(&tar),
            Err(TarError::BadHeader { offset: 0 })
        ));
    }
}
