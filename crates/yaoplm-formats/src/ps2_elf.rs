//! Loader ELFs: the version string inside an OPL-family ELF, also when
//! ps2-packer packed it (LDR-4, LIB-1).
//!
//! Format reference: `docs/research/16-loader-signs.md` §1.6. The version is
//! `OPL_VERSION` from upstream's `Makefile` (`v1.1.0`,
//! `v1.2.0-Beta-<N>-<hash>`, plus a fork's `LOCALVERSION` such as `-yaopl`;
//! YAOPL's own `v2.0.0-nightly.<N>-<hash>-yaopl`, `v2.0.0-yaopl`),
//! compiled in as a C string and shown by `src/gui.c` ("Open PS2 Loader %s")
//! of ps2homebrew/Open-PS2-Loader@b3a21997. Release ELFs are packed by
//! `ps2-packer` (`Makefile` `EE_BIN_PACKED`); checked on 2026-10-25 on the
//! release ELFs of upstream 1.0.0, 1.1.0 and Beta-2271, YAOPL, RiptOPL,
//! wOPL and other forks:
//! - one `PT_LOAD` segment holds the unpacker stub and starts with six
//!   little-endian u32: entry of the real program, a count, unpacked size,
//!   zero-fill size, load address, packed size;
//! - right after them comes a raw LZMA stream: the properties byte, the
//!   dictionary size as a little-endian u32, then the data, without the
//!   unpacked size of the `.lzma` format. It ends with an end marker one
//!   byte before the unpacked size in the header.
//!
//! Nothing is ever run. The decoder never makes more than the EE's 32 MiB of
//! RAM, the most a PS2 program can be. A file whose stream does not decode,
//! or that holds no version string, gives `None`: "version unknown".

use std::borrow::Cow;
use std::io::Read;

use lzma_rust2::LzmaReader;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS32: u8 = 1;
const ELFDATA2LSB: u8 = 1;
const EM_MIPS: u16 = 8;
const PT_LOAD: u32 = 1;
/// The EE's main RAM: no PS2 program unpacks to more.
const MAX_UNPACKED: u32 = 32 << 20;
/// ps2-packer's header before the LZMA stream: six u32.
const PACKER_HEADER: usize = 24;
/// LZMA properties: `(pb * 5 + lp) * 9 + lc` with pb, lp ≤ 4 and lc ≤ 8.
const MAX_PROPS: u8 = 9 * 5 * 5;
const MAX_VERSION_LEN: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ElfError {
    #[error("not a PS2 ELF (32-bit little-endian MIPS)")]
    NotPs2Elf,
}

/// The version string found in an ELF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfVersion {
    /// `v1.1.0`, `v1.2.0-Beta-2271-b3a2199`, `v1.2.0-Beta-2370-33a2b19-yaopl`.
    pub version: String,
    /// Found inside ps2-packer's LZMA stream, not in plain text.
    pub packed: bool,
}

/// The program inside a loader ELF: the unpacked payload when ps2-packer
/// packed it, else the file itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program<'a> {
    pub bytes: Cow<'a, [u8]>,
    pub packed: bool,
}

impl Program<'_> {
    /// Whether the program holds `needle`, e.g. a settings file name only
    /// one loader family uses.
    pub fn contains(&self, needle: &[u8]) -> bool {
        !needle.is_empty() && self.bytes.windows(needle.len()).any(|w| w == needle)
    }
}

/// The program of a loader ELF, unpacking it when ps2-packer packed it.
/// A packed file's stub may carry strings of its own, so when the payload
/// unpacks only the payload counts.
pub fn program(data: &[u8]) -> Result<Program<'_>, ElfError> {
    let elf = Elf::parse(data)?;
    Ok(match elf.unpack(data) {
        Some(unpacked) => Program {
            bytes: Cow::Owned(unpacked),
            packed: true,
        },
        None => Program {
            bytes: Cow::Borrowed(data),
            packed: false,
        },
    })
}

/// The version string of a loader ELF, unpacking it when ps2-packer packed it.
pub fn read_version(data: &[u8]) -> Result<Option<ElfVersion>, ElfError> {
    let program = program(data)?;
    Ok(find_version(&program.bytes).map(|version| ElfVersion {
        version,
        packed: program.packed,
    }))
}

struct Elf {
    phoff: usize,
    phentsize: usize,
    phnum: usize,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

impl Elf {
    fn parse(data: &[u8]) -> Result<Self, ElfError> {
        let ident = data.get(..6).ok_or(ElfError::NotPs2Elf)?;
        if ident[..4] != ELF_MAGIC || ident[4] != ELFCLASS32 || ident[5] != ELFDATA2LSB {
            return Err(ElfError::NotPs2Elf);
        }
        if u16_at(data, 0x12) != Some(EM_MIPS) {
            return Err(ElfError::NotPs2Elf);
        }
        let header = || -> Option<Self> {
            Some(Self {
                phoff: usize::try_from(u32_at(data, 0x1C)?).ok()?,
                phentsize: usize::from(u16_at(data, 0x2A)?),
                phnum: usize::from(u16_at(data, 0x2C)?),
            })
        };
        header().ok_or(ElfError::NotPs2Elf)
    }

    /// The program ps2-packer packed, or `None` when this is not such a file
    /// or its stream does not decode.
    fn unpack(&self, data: &[u8]) -> Option<Vec<u8>> {
        // ps2-packer writes one segment: the stub with the payload.
        if self.phnum != 1 || self.phentsize < 32 {
            return None;
        }
        if u32_at(data, self.phoff)? != PT_LOAD {
            return None;
        }
        let start = usize::try_from(u32_at(data, self.phoff + 4)?).ok()?;
        let unpacked = u32_at(data, start + 8)?;
        if unpacked == 0 || unpacked > MAX_UNPACKED {
            return None;
        }
        let props = *data.get(start + PACKER_HEADER)?;
        let dict_size = u32_at(data, start + PACKER_HEADER + 1)?;
        if props >= MAX_PROPS || dict_size > MAX_UNPACKED {
            return None;
        }
        let stream = data.get(start + PACKER_HEADER + 5..)?;
        let mut reader =
            LzmaReader::new_with_props(stream, u64::from(unpacked), props, dict_size, None).ok()?;
        // The streams end with an end marker one byte short of the size in
        // the header (all the release ELFs read, 2026-10-25), which the
        // decoder reports as an error: hand out the bytes before it first.
        reader.set_defer_errors(true);
        let mut out = Vec::new();
        let mut reader = reader.take(u64::from(unpacked));
        let mut chunk = vec![0; 64 << 10];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => out.extend_from_slice(&chunk[..n]),
                // A damaged or cut stream: what came out so far still counts.
                Err(_) => break,
            }
        }
        (!out.is_empty()).then_some(out)
    }
}

/// The best version string among the C strings of `data`, or after a `/`
/// in one (`OPL/v…`, `RiptOPL/v…`: the HTTP user agent, the only copy in
/// RiptOPL's RetroAchievements builds). Three numbers beat two, a beta build
/// beats a release, a string of its own beats one after `/`, a longer
/// string beats a shorter one.
pub fn find_version(data: &[u8]) -> Option<String> {
    let mut best: Option<((u8, bool, usize), &str)> = None;
    let mut at = 0;
    while let Some(offset) = memchr(b'v', &data[at..]) {
        let start = at + offset;
        at = start + 1;
        let own = start == 0 || data[start - 1] == 0;
        if !own && data[start - 1] != b'/' {
            continue;
        }
        let Some(len) = memchr(0, &data[start..data.len().min(start + MAX_VERSION_LEN + 1)]) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&data[start..start + len]) else {
            continue;
        };
        let Some(rank) = version_rank(text) else {
            continue;
        };
        let score = (rank, own, text.len());
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, text));
        }
    }
    best.map(|(_, text)| text.to_owned())
}

fn memchr(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// How much `text` looks like a loader version, or `None` when it does not:
/// `v` with two or three numbers, then dash-separated parts of letters,
/// digits, `_`, `=` and `.`. Two numbers need a part (`v1.2-beta-468`).
fn version_rank(text: &str) -> Option<u8> {
    let rest = text.strip_prefix('v')?;
    let mut parts = rest.split('-');
    let numbers: Vec<&str> = parts.next()?.split('.').collect();
    if !(2..=3).contains(&numbers.len())
        || numbers
            .iter()
            .any(|n| n.is_empty() || n.len() > 4 || !n.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let suffix: Vec<&str> = parts.collect();
    if suffix.iter().any(|p| {
        p.is_empty()
            || p.len() > 32
            || !p
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'=' | b'.'))
    }) {
        return None;
    }
    if numbers.len() == 2 && suffix.is_empty() {
        return None;
    }
    let beta = suffix
        .first()
        .is_some_and(|p| p.eq_ignore_ascii_case("beta"));
    Some(u8::from(numbers.len() == 3) * 2 + u8::from(beta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lzma_rust2::{LzmaOptions, LzmaWriter};
    use std::io::Write;

    /// A minimal 32-bit little-endian MIPS ELF with one `PT_LOAD` segment at
    /// 0x60 holding `segment`; `phnum` program headers are declared.
    fn elf(segment: &[u8], phnum: u16) -> Vec<u8> {
        let mut out = vec![0; 0x60];
        out[..4].copy_from_slice(&ELF_MAGIC);
        out[4] = ELFCLASS32;
        out[5] = ELFDATA2LSB;
        out[6] = 1;
        out[0x10..0x12].copy_from_slice(&2u16.to_le_bytes());
        out[0x12..0x14].copy_from_slice(&EM_MIPS.to_le_bytes());
        out[0x1C..0x20].copy_from_slice(&0x34u32.to_le_bytes());
        out[0x2A..0x2C].copy_from_slice(&32u16.to_le_bytes());
        out[0x2C..0x2E].copy_from_slice(&phnum.to_le_bytes());
        // Program header at 0x34: PT_LOAD, offset 0x60.
        out[0x34..0x38].copy_from_slice(&PT_LOAD.to_le_bytes());
        out[0x38..0x3C].copy_from_slice(&0x60u32.to_le_bytes());
        out.extend_from_slice(segment);
        out
    }

    /// The program with ps2-packer's header and a raw LZMA stream, as in the
    /// release ELFs: an end marker, and a size in the header one byte more.
    fn packed(program: &[u8]) -> Vec<u8> {
        packed_with(program, true)
    }

    fn packed_with(program: &[u8], end_marker: bool) -> Vec<u8> {
        let options = LzmaOptions::with_preset(6);
        let mut stream = Vec::new();
        let mut writer = LzmaWriter::new_no_header(&mut stream, &options, end_marker).unwrap();
        writer.write_all(program).unwrap();
        let props = writer.props();
        writer.finish().unwrap();
        let mut segment = Vec::new();
        for word in [
            0x0010_0000,
            1,
            program.len() as u32 + u32::from(end_marker),
            0,
            0x0010_0000,
            stream.len() as u32,
        ] {
            segment.extend_from_slice(&u32::to_le_bytes(word));
        }
        segment.push(props);
        segment.extend_from_slice(&options.dict_size.to_le_bytes());
        segment.extend_from_slice(&stream);
        // The stub follows the payload in the real files.
        segment.extend_from_slice(b"stub v0.4.6\0LZMA");
        elf(&segment, 1)
    }

    fn program(version: &str) -> Vec<u8> {
        let mut out = vec![0x11; 300_000];
        out.extend_from_slice(b"\0Open PS2 Loader %s\0");
        out.extend_from_slice(version.as_bytes());
        out.push(0);
        out.extend(std::iter::repeat_n(0x22, 50_000));
        out
    }

    #[test]
    fn plain_and_packed() {
        let plain = elf(&program("v1.1.0"), 2);
        assert_eq!(
            read_version(&plain).unwrap(),
            Some(ElfVersion {
                version: "v1.1.0".into(),
                packed: false
            })
        );
        // The version sits in the last 64 KiB, where the end marker comes.
        let yaopl = packed(&program("v1.2.0-Beta-2370-33a2b19-yaopl"));
        assert_eq!(
            read_version(&yaopl).unwrap(),
            Some(ElfVersion {
                version: "v1.2.0-Beta-2370-33a2b19-yaopl".into(),
                packed: true
            })
        );
        // YAOPL's own versions (ADR-0051).
        let nightly = packed(&program("v2.0.0-nightly.12-0123abc-yaopl"));
        assert_eq!(
            read_version(&nightly).unwrap(),
            Some(ElfVersion {
                version: "v2.0.0-nightly.12-0123abc-yaopl".into(),
                packed: true
            })
        );
        // A stream without an end marker, its exact size in the header.
        let exact = packed_with(&program("v1.1.0"), false);
        assert_eq!(read_version(&exact).unwrap().unwrap().version, "v1.1.0");
    }

    #[test]
    fn the_stub_does_not_count_when_the_payload_unpacks() {
        let mut payload = vec![0x33; 10_000];
        payload.extend_from_slice(b"\0no version here\0");
        assert_eq!(read_version(&packed(&payload)).unwrap(), None);
    }

    #[test]
    fn not_an_elf() {
        assert_eq!(read_version(b""), Err(ElfError::NotPs2Elf));
        assert_eq!(read_version(b"MZ\x90\0"), Err(ElfError::NotPs2Elf));
        let mut x86 = elf(b"\0v1.1.0\0", 2);
        x86[0x12] = 3;
        assert_eq!(read_version(&x86), Err(ElfError::NotPs2Elf));
    }

    #[test]
    fn damaged_files_give_no_version_and_never_panic() {
        let good = packed(&program("v1.2.0-Beta-2271-b3a2199"));
        // Cut before the version: nothing; cut anywhere: no panic.
        assert_eq!(read_version(&good[..good.len() / 3]).unwrap(), None);
        for len in (0..good.len()).step_by(997) {
            let _ = read_version(&good[..len]);
        }
        // Properties, dictionary and sizes out of range.
        let props = 0x60 + PACKER_HEADER;
        let mut bad = good.clone();
        bad[props] = MAX_PROPS;
        assert_eq!(read_version(&bad).unwrap(), None);
        let mut bad = good.clone();
        bad[props + 1..props + 5].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_version(&bad).unwrap(), None);
        let mut bad = good.clone();
        bad[0x68..0x6C].copy_from_slice(&(MAX_UNPACKED + 1).to_le_bytes());
        assert_eq!(read_version(&bad).unwrap(), None);
        // A segment offset past the end of the file.
        let mut bad = good.clone();
        bad[0x38..0x3C].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_version(&bad).unwrap(), None);
        // Flipped bytes in the stream: maybe a version, never a panic.
        let mut bad = good;
        for i in (props + 5..bad.len()).step_by(4093) {
            bad[i] ^= 0x5A;
        }
        let _ = read_version(&bad);
    }

    #[test]
    fn version_strings() {
        for good in [
            "v1.1.0",
            "v1.0.0",
            "v1.2.0-Beta-2271-b3a2199",
            "v1.2.0-Beta-2370-33a2b19-yaopl",
            "v1.2.0-Beta-2370-33a2b19-yaopl-nosio2man",
            "v1.2-beta-468",
            "v1.2.2",
            "v2.0.0-nightly.12-0123abc-yaopl",
            "v2.0.0-rc1-yaopl",
            "v2.0.0-yaopl",
        ] {
            assert!(version_rank(good).is_some(), "{good}");
        }
        for bad in [
            "v1",
            "v1.0",
            "version",
            "v1.2.3.4",
            "v1.2.0-",
            "v1.2.0-Beta 2",
            "vx.y.z",
            "v12345.0.0",
        ] {
            assert!(version_rank(bad).is_none(), "{bad}");
        }
        // The release string wins over a two-number one, a beta over a release.
        let data = b"\0v1.2-rc\0v1.1.0\0";
        assert_eq!(find_version(data).as_deref(), Some("v1.1.0"));
        let data = b"\0v1.1.0\0v1.2.0-Beta-2271-b3a2199\0";
        assert_eq!(
            find_version(data).as_deref(),
            Some("v1.2.0-Beta-2271-b3a2199")
        );
        // Inside a longer string it is not a version.
        assert_eq!(find_version(b"\0usbd v1.1.0\0"), None);
        // After a `/`, as in the user agent; a string of its own wins.
        let agent = b"\0RiptOPL/v1.2.0-Beta-4257-RA-Korium-55261de-OFFICIALROLLING-RA-Korium\0";
        assert_eq!(
            find_version(agent).as_deref(),
            Some("v1.2.0-Beta-4257-RA-Korium-55261de-OFFICIALROLLING-RA-Korium")
        );
        let both = b"\0OPL/v1.2.0-Beta-2271-b3a2199-x\0v1.2.0-Beta-2271-b3a2199\0";
        assert_eq!(
            find_version(both).as_deref(),
            Some("v1.2.0-Beta-2271-b3a2199")
        );
    }
}
