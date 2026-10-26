//! `SYSTEM.CNF`: the game ID ("startup") exactly as OPL derives it (IMP-2).
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.3. Mirrors
//! `ps2cnfGetBootFile()` in `src/ps2cnf.c` and `GetStartupExecName()` in
//! `src/supportbase.c` of ps2homebrew/Open-PS2-Loader@b3a21997
//! (reimplemented from the logic, ADR-0002). RiptOPL, wOPL, OPL-Evolution and
//! the ps2-mmce fork carry the same code.
//!
//! What OPL does, including the parts that look like accidents:
//! - Reads at most 1023 bytes.
//! - Looks for `BOOT2` at the start of a line, case-sensitively, after
//!   skipping whitespace. Whitespace includes newlines, so blank lines are
//!   skipped; `BOOT2X` also matches the prefix and then fails on `=`.
//! - Skips whitespace (again including newlines), expects `=`, skips more
//!   whitespace and takes the first run of printable characters, at most 64.
//!   A control character or a byte >= 0x80 ends the run.
//! - From that value it skips everything up to the first `:`, then any `\`,
//!   and copies up to `;`, keeping at most 11 characters.
//! - Any failure hides the image from the game list. A NUL byte before the
//!   `BOOT2` line counts as end of file.
//!
//! [`parse_info`] is a separate, lenient reader for display (VER, VMODE, the
//! PS1 `BOOT` key). OPL never reads those.

/// OPL reads at most this many bytes (`CNF_LEN_MAX - 1`).
pub const CNF_READ_MAX: usize = 1023;
/// Size of OPL's boot path buffer (`CNF_PATH_LEN_MAX`).
pub const BOOT_PATH_MAX: usize = 64;
/// Longest game ID OPL keeps (`GAME_STARTUP_MAX - 1`).
pub const STARTUP_MAX: usize = 11;

/// Why OPL would not list an image. Every variant hides the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CnfError {
    #[error("SYSTEM.CNF has no BOOT2 line (PS1 disc, video DVD or damaged image)")]
    NoBoot2,
    /// OPL stops at a NUL byte as if the file ended there.
    #[error("SYSTEM.CNF has a NUL byte before its BOOT2 line")]
    NulByte,
    #[error("SYSTEM.CNF ends inside the BOOT2 line")]
    UnexpectedEof,
    /// OPL then parses from a null pointer; the outcome is unpredictable.
    #[error("BOOT2 is not followed by '='")]
    MissingEquals,
    #[error("BOOT2 value has no ':' device prefix, e.g. cdrom0:")]
    MissingColon,
    #[error("BOOT2 value names no file")]
    EmptyStartup,
    /// The 64-byte boot path filled up without a terminator, and the result
    /// depends on memory past OPL's buffer.
    #[error("BOOT2 value is too long for OPL's 64-byte buffer")]
    Unpredictable,
}

/// The `BOOT2` value as OPL copied it, e.g. `cdrom0:\SLUS_200.62;1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootPath {
    /// Printable ASCII only, at most [`BOOT_PATH_MAX`] bytes.
    pub value: String,
    /// The copy filled the whole buffer, so OPL's string is not terminated.
    pub unterminated: bool,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

fn is_graph(b: u8) -> bool {
    (0x21..=0x7E).contains(&b)
}

enum Token {
    Match(usize),
    NoMatch,
    /// End of data or a NUL byte, at this position.
    Eof(usize),
}

struct Cnf<'a> {
    buf: &'a [u8],
}

impl Cnf<'_> {
    /// OPL NUL-terminates its copy, so reading at the end yields 0.
    fn at(&self, pos: usize) -> u8 {
        self.buf.get(pos).copied().unwrap_or(0)
    }

    fn end(&self) -> usize {
        self.buf.len()
    }

    fn skip_space(&self, mut pos: usize) -> usize {
        while pos < self.end() && is_space(self.at(pos)) {
            pos += 1;
        }
        pos
    }

    /// `CNFGetToken`.
    fn token(&self, pos: usize, key: &[u8]) -> Token {
        let mut pos = self.skip_space(pos);
        for &k in key {
            if pos >= self.end() || self.at(pos) == 0 {
                return Token::Eof(pos);
            }
            if self.at(pos) != k {
                return Token::NoMatch;
            }
            pos += 1;
        }
        Token::Match(pos)
    }

    /// `CNFAdvanceLine`: the byte after the next LF, unless that is the end.
    fn next_line(&self, start: usize) -> Option<usize> {
        let lf = (start..self.end()).find(|&p| self.at(p) == b'\n')?;
        (lf + 1 < self.end()).then_some(lf + 1)
    }
}

/// Finds the `BOOT2` value like `ps2cnfGetBootFile()`.
pub fn parse_boot2(cnf: &[u8]) -> Result<BootPath, CnfError> {
    let cnf = Cnf {
        buf: &cnf[..cnf.len().min(CNF_READ_MAX)],
    };

    let mut line = 0;
    let after_key = loop {
        match cnf.token(line, b"BOOT2") {
            Token::Match(pos) => break pos,
            Token::Eof(pos) if pos < cnf.end() => return Err(CnfError::NulByte),
            Token::Eof(_) => return Err(CnfError::NoBoot2),
            Token::NoMatch => line = cnf.next_line(line).ok_or(CnfError::NoBoot2)?,
        }
    };
    let mut pos = match cnf.token(after_key, b"=") {
        Token::Match(pos) => pos,
        Token::Eof(_) => return Err(CnfError::UnexpectedEof),
        Token::NoMatch => return Err(CnfError::MissingEquals),
    };

    // `CNFGetKey`: this whitespace skip has no end check, the NUL stops it.
    while is_space(cnf.at(pos)) {
        pos += 1;
    }
    if cnf.at(pos) == 0 {
        return Err(CnfError::UnexpectedEof);
    }
    let mut value = String::new();
    for _ in 0..BOOT_PATH_MAX {
        let b = cnf.at(pos);
        if b == 0 || is_space(b) {
            break;
        }
        if is_graph(b) {
            value.push(char::from(b));
            pos += 1;
        }
        // Any other byte: OPL neither copies nor advances, so the loop just
        // runs out of iterations.
    }
    let unterminated = value.len() == BOOT_PATH_MAX;
    Ok(BootPath {
        value,
        unterminated,
    })
}

/// Derives the game ID from a boot path like `GetStartupExecName()`.
pub fn startup_from_boot_path(boot: &BootPath) -> Result<String, CnfError> {
    let bytes = boot.value.as_bytes();
    let colon = match bytes.iter().position(|&b| b == b':') {
        Some(colon) => colon,
        None if boot.unterminated => return Err(CnfError::Unpredictable),
        None => return Err(CnfError::MissingColon),
    };
    let mut start = colon + 1;
    while bytes.get(start) == Some(&b'\\') {
        start += 1;
    }
    let rest = &bytes[start.min(bytes.len())..];
    let len = rest.iter().position(|&b| b == b';').unwrap_or(rest.len());
    // Past the end of an unterminated value OPL reads stack memory.
    if boot.unterminated && len < STARTUP_MAX && len == rest.len() {
        return Err(CnfError::Unpredictable);
    }
    // A `;` inside the buffer ends the ID even when the value fills it.
    if len == 0 {
        return Err(CnfError::EmptyStartup);
    }
    Ok(boot.value[start..start + len.min(STARTUP_MAX)].to_owned())
}

/// The game ID OPL shows for an image with this `SYSTEM.CNF`.
pub fn opl_startup(cnf: &[u8]) -> Result<String, CnfError> {
    startup_from_boot_path(&parse_boot2(cnf)?)
}

/// Lenient view of `SYSTEM.CNF` for display. Keys match case-insensitively
/// and values are trimmed, unlike OPL.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CnfInfo {
    pub boot2: Option<String>,
    /// PS1 discs boot through `BOOT` instead of `BOOT2`.
    pub boot: Option<String>,
    pub ver: Option<String>,
    pub vmode: Option<String>,
}

pub fn parse_info(cnf: &[u8]) -> CnfInfo {
    let text = String::from_utf8_lossy(&cnf[..cnf.len().min(CNF_READ_MAX)]);
    let mut info = CnfInfo::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = Some(value.trim().to_owned());
        let slot = match key.trim().to_ascii_uppercase().as_str() {
            "BOOT2" => &mut info.boot2,
            "BOOT" => &mut info.boot,
            "VER" => &mut info.ver,
            "VMODE" => &mut info.vmode,
            _ => continue,
        };
        if slot.is_none() {
            *slot = value;
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(cnf: &str) -> Result<String, CnfError> {
        opl_startup(cnf.as_bytes())
    }

    #[test]
    fn retail_disc() {
        let cnf = "BOOT2 = cdrom0:\\SLUS_200.62;1\r\nVER = 1.10\r\nVMODE = NTSC\r\n";
        assert_eq!(id(cnf).as_deref(), Ok("SLUS_200.62"));
        let info = parse_info(cnf.as_bytes());
        assert_eq!(info.boot2.as_deref(), Some("cdrom0:\\SLUS_200.62;1"));
        assert_eq!(info.ver.as_deref(), Some("1.10"));
        assert_eq!(info.vmode.as_deref(), Some("NTSC"));
        assert_eq!(info.boot, None);
    }

    #[test]
    fn line_endings_and_spacing() {
        assert_eq!(
            id("BOOT2=cdrom0:\\SLES_123.45;1").as_deref(),
            Ok("SLES_123.45")
        );
        assert_eq!(
            id("BOOT2\t=\tcdrom0:\\SLES_123.45;1\n").as_deref(),
            Ok("SLES_123.45")
        );
        assert_eq!(id("\r\n\r\n  BOOT2 = cdrom0:\\A;1").as_deref(), Ok("A"));
        // Whitespace skipping crosses newlines, around '=' as well.
        assert_eq!(id("BOOT2\n=\ncdrom0:\\B;1").as_deref(), Ok("B"));
        // No ';': the ID runs to the end of the value.
        assert_eq!(
            id("BOOT2 = cdrom0:\\SCUS_971.13").as_deref(),
            Ok("SCUS_971.13")
        );
    }

    #[test]
    fn key_is_case_sensitive_and_line_anchored() {
        assert_eq!(id("boot2 = cdrom0:\\SLUS_200.62;1"), Err(CnfError::NoBoot2));
        assert_eq!(id("Boot2 = cdrom0:\\SLUS_200.62;1"), Err(CnfError::NoBoot2));
        assert_eq!(
            id("VMODE = NTSC BOOT2 = cdrom0:\\X;1"),
            Err(CnfError::NoBoot2)
        );
        // Later lines are still searched.
        assert_eq!(id("VMODE = PAL\nBOOT2 = cdrom0:\\X;1").as_deref(), Ok("X"));
    }

    #[test]
    fn ps1_and_empty_discs_are_hidden() {
        assert_eq!(
            id("BOOT = cdrom:\\SLUS_005.94;1\r\n"),
            Err(CnfError::NoBoot2)
        );
        assert_eq!(id(""), Err(CnfError::NoBoot2));
        assert_eq!(id("\n"), Err(CnfError::NoBoot2));
        assert_eq!(id("VMODE = NTSC\n\n"), Err(CnfError::NoBoot2));
    }

    #[test]
    fn utf8_bom_is_not_skipped() {
        // Unlike OPL's CFG reader, ps2cnf.c has no BOM handling.
        let bom = b"\xEF\xBB\xBF".as_slice();
        let first_line = [bom, b"BOOT2 = cdrom0:\\A;1"].concat();
        assert_eq!(opl_startup(&first_line), Err(CnfError::NoBoot2));
        let second_line = [bom, b"\nBOOT2 = cdrom0:\\A;1"].concat();
        assert_eq!(opl_startup(&second_line).as_deref(), Ok("A"));
    }

    #[test]
    fn prefix_match_needs_equals() {
        assert_eq!(id("BOOT2X = cdrom0:\\A;1"), Err(CnfError::MissingEquals));
        assert_eq!(id("BOOT2 cdrom0:\\A;1"), Err(CnfError::MissingEquals));
    }

    #[test]
    fn eof_and_nul() {
        assert_eq!(id("BOO"), Err(CnfError::NoBoot2));
        assert_eq!(id("BOOT2"), Err(CnfError::UnexpectedEof));
        assert_eq!(id("BOOT2 =   "), Err(CnfError::UnexpectedEof));
        // A NUL where a line should start reads as end of file.
        assert_eq!(id("\0\nBOOT2 = cdrom0:\\A;1"), Err(CnfError::NulByte));
        assert_eq!(id("BO\0T2 = cdrom0:\\A;1"), Err(CnfError::NulByte));
        // After the BOOT2 line, NUL padding is harmless.
        assert_eq!(id("BOOT2 = cdrom0:\\A;1\n\0\0\0").as_deref(), Ok("A"));
    }

    #[test]
    fn device_prefix() {
        assert_eq!(id("BOOT2 = SLUS_200.62;1"), Err(CnfError::MissingColon));
        assert_eq!(id("BOOT2 = cdrom0:;1"), Err(CnfError::EmptyStartup));
        assert_eq!(id("BOOT2 = cdrom0:\\\\\\;1"), Err(CnfError::EmptyStartup));
        assert_eq!(
            id("BOOT2 = cdrom:\\SLPM_123.45;1").as_deref(),
            Ok("SLPM_123.45")
        );
        // Only backslashes are skipped.
        assert_eq!(
            id("BOOT2 = cdrom0:/SLUS_200.62;1").as_deref(),
            Ok("/SLUS_200.6")
        );
        // Only the first ':' counts.
        assert_eq!(id("BOOT2 = a:b:c;1").as_deref(), Ok("b:c"));
    }

    #[test]
    fn subfolder_and_long_ids_truncate_to_11() {
        assert_eq!(
            id("BOOT2 = cdrom0:\\DATA\\MAIN.ELF;1").as_deref(),
            Ok("DATA\\MAIN.E")
        );
        assert_eq!(
            id("BOOT2 = cdrom0:\\SLUS_200.6299;1").as_deref(),
            Ok("SLUS_200.62")
        );
        assert_eq!(
            id("BOOT2 = cdrom0:\\ABCDEFGHIJKLMNOP").as_deref(),
            Ok("ABCDEFGHIJK")
        );
    }

    #[test]
    fn value_stops_at_space_or_control_byte() {
        assert_eq!(id("BOOT2 = cdrom0:\\MY GAME.ELF;1").as_deref(), Ok("MY"));
        assert_eq!(id("BOOT2 = cdrom0:\\AB\x01CD;1").as_deref(), Ok("AB"));
        assert_eq!(
            opl_startup(b"BOOT2 = cdrom0:\\AB\xC3\xA9;1").as_deref(),
            Ok("AB")
        );
    }

    #[test]
    fn only_first_1023_bytes_are_read() {
        let mut cnf = "\n".repeat(1000);
        cnf.push_str("BOOT2 = cdrom0:\\SLUS_200.62;1");
        // The ID would need byte 1031; OPL stops at 1023.
        assert_eq!(id(&cnf).as_deref(), Ok("SLUS_20"));
        let mut cnf = "\n".repeat(1020);
        cnf.push_str("BOOT2 = cdrom0:\\A;1");
        assert_eq!(id(&cnf), Err(CnfError::NoBoot2));
    }

    #[test]
    fn sixty_four_byte_values() {
        let value = format!("cdrom0:\\{}", "A".repeat(BOOT_PATH_MAX - 8));
        let boot = parse_boot2(format!("BOOT2 = {value} rest").as_bytes()).unwrap();
        assert!(boot.unterminated);
        assert_eq!(boot.value.len(), BOOT_PATH_MAX);
        // The ID fits inside the buffer, so the result is still defined.
        assert_eq!(startup_from_boot_path(&boot).as_deref(), Ok("AAAAAAAAAAA"));

        let no_colon = parse_boot2(format!("BOOT2 = {}", "A".repeat(70)).as_bytes()).unwrap();
        assert_eq!(
            startup_from_boot_path(&no_colon),
            Err(CnfError::Unpredictable)
        );

        let short_tail = format!("BOOT2 = {}:\\AB", "A".repeat(BOOT_PATH_MAX - 4));
        let boot = parse_boot2(short_tail.as_bytes()).unwrap();
        assert_eq!(startup_from_boot_path(&boot), Err(CnfError::Unpredictable));
    }

    #[test]
    fn a_full_buffer_with_an_empty_id_hides_the_game() {
        // OPL finds the `;` inside its 64-byte buffer, counts a length of 0
        // and hides the image (`GetStartupExecName`, src/supportbase.c).
        for value in [
            format!("cdrom0:;{}", "1".repeat(BOOT_PATH_MAX - 8)),
            format!("cdrom0:\\\\;{}", "1".repeat(BOOT_PATH_MAX - 10)),
        ] {
            let boot = parse_boot2(format!("BOOT2 = {value} rest").as_bytes()).unwrap();
            assert!(boot.unterminated);
            assert_eq!(
                startup_from_boot_path(&boot),
                Err(CnfError::EmptyStartup),
                "{value}"
            );
        }
        // Backslashes to the end of the buffer: OPL skips on past it.
        let value = format!("cdrom0:{}", "\\".repeat(BOOT_PATH_MAX - 7));
        let boot = parse_boot2(format!("BOOT2 = {value}").as_bytes()).unwrap();
        assert!(boot.unterminated);
        assert_eq!(startup_from_boot_path(&boot), Err(CnfError::Unpredictable));
    }

    #[test]
    fn info_reads_ps1_boot_key() {
        let info = parse_info(b"BOOT = cdrom:\\SCUS_944.26;1\r\nTCB = 4\r\n");
        assert_eq!(info.boot.as_deref(), Some("cdrom:\\SCUS_944.26;1"));
        assert_eq!(info.boot2, None);
    }
}
