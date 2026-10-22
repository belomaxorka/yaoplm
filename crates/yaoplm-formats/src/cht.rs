//! Cheat files `CHT/<ID>.cht` (PS2RD raw codes): the parser model.
//!
//! Format reference: `docs/research/02-opl-formats.md` §8. Mirrors
//! `parse_buf()`/`set_cheats_list()` in `src/cheatman.c` and
//! `normalizeCheatName()` in `src/cheatconfig.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997 (cheatman.c itself is GPL-3 code
//! ported from PS2rd).
//!
//! - Lines end at LF and are cut to 255 bytes; the rest of an over-long line
//!   is parsed as the next line, minus one byte.
//! - `//` or `#` anywhere starts a comment; then the line is trimmed.
//! - A line of exactly 16 hex digits (whitespace allowed) is a code;
//!   `00000000 00000000` is ignored. Any other non-empty line names the codes
//!   after it; a name followed by another name is replaced.
//! - Quirk: a comment line with leading blanks trims to nothing but is not
//!   empty, so it becomes a cheat named by those blanks.
//! - Names `mastercode`, `master code`, `enable code (must be on)` (case and
//!   spacing ignored) are always on.
//! - Upstream accepts 251 cheats into a 250-slot array (off-by-one); the
//!   engine receives at most 254 address/value pairs in total, and stops a
//!   cheat at its first code with address 0.

/// `MAX_CODES`: cheat entries.
pub const MAX_CHEATS: usize = 250;
/// `CHEAT_NAME_MAX`.
pub const NAME_MAX: usize = 128;
/// `CHEAT_LINE_MAX`.
pub const LINE_MAX: usize = 255;
/// `MAX_CHEATLIST`: codes one cheat can hold.
pub const MAX_CODES_PER_CHEAT: usize = 510;
/// `MAX_CHEATLIST / 2 - 1`: pairs left after the 0,0 terminator.
pub const MAX_ACTIVE_PAIRS: usize = 254;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    pub addr: u32,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cheat {
    pub name: String,
    pub codes: Vec<Code>,
}

impl Cheat {
    pub fn is_master_code(&self) -> bool {
        is_master_code_name(&self.name)
    }

    /// Codes the engine gets: up to the first one with address 0.
    pub fn effective_codes(&self) -> &[Code] {
        let end = self
            .codes
            .iter()
            .position(|c| c.addr == 0)
            .unwrap_or(self.codes.len());
        &self.codes[..end]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChtWarning {
    /// Codes before the first name are dropped.
    CodesWithoutName(usize),
    /// More than 250 cheats: OPL writes past its array.
    TooManyCheats,
    /// A comment line indented with blanks renamed the next cheat to blanks.
    BlankName {
        line: usize,
    },
    LineTooLong {
        line: usize,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChtFile {
    pub cheats: Vec<Cheat>,
    pub warnings: Vec<ChtWarning>,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

fn is_graph(b: u8) -> bool {
    (0x21..=0x7E).contains(&b)
}

fn parse_code(line: &[u8]) -> Option<Code> {
    let mut digits = Vec::with_capacity(16);
    for &b in line {
        if b.is_ascii_hexdigit() {
            digits.push(b);
            if digits.len() > 16 {
                return None;
            }
        } else if !is_space(b) {
            return None;
        }
    }
    if digits.len() != 16 {
        return None;
    }
    let hex = std::str::from_utf8(&digits).ok()?;
    Some(Code {
        addr: u32::from_str_radix(&hex[..8], 16).ok()?,
        value: u32::from_str_radix(&hex[8..], 16).ok()?,
    })
}

fn strip_comment(line: &[u8]) -> &[u8] {
    let cut = (0..line.len())
        .find(|&i| line[i] == b'#' || line[i..].starts_with(b"//"))
        .unwrap_or(line.len());
    &line[..cut]
}

/// `trim_str()`: leaves an all-blank string untouched.
fn trim(line: &[u8]) -> &[u8] {
    if !line.iter().copied().any(is_graph) {
        return line;
    }
    let start = line.iter().position(|&b| !is_space(b)).unwrap_or(0);
    let end = line
        .iter()
        .rposition(|&b| !is_space(b))
        .map_or(start, |e| e + 1);
    &line[start..end]
}

impl ChtFile {
    pub fn parse(bytes: &[u8]) -> Self {
        // The file is read as a C string.
        let bytes = &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())];
        let mut file = Self::default();
        let mut pending_name: Option<String> = None;
        let mut orphans = 0;
        let mut overflow = false;
        let mut pos = 0;
        let mut line_no = 0;
        while pos < bytes.len() {
            line_no += 1;
            let rest = &bytes[pos..];
            let mut len = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
            if len > LINE_MAX {
                len = LINE_MAX;
                file.warnings
                    .push(ChtWarning::LineTooLong { line: line_no });
            }
            let raw = &rest[..len];
            pos += len + 1;
            if !raw.iter().copied().any(is_graph) {
                continue;
            }
            let line = trim(strip_comment(raw));
            match parse_code(line) {
                None if !line.is_empty() => {
                    let name = &line[..line.len().min(NAME_MAX)];
                    if !name.iter().copied().any(is_graph) {
                        file.warnings.push(ChtWarning::BlankName { line: line_no });
                    }
                    pending_name = Some(String::from_utf8_lossy(name).into_owned());
                }
                Some(code) if code != (Code { addr: 0, value: 0 }) => {
                    if let Some(name) = pending_name.take_if(|n| !n.is_empty()) {
                        if file.cheats.len() < MAX_CHEATS {
                            file.cheats.push(Cheat {
                                name,
                                codes: Vec::new(),
                            });
                        } else {
                            overflow = true;
                        }
                    }
                    match file.cheats.last_mut() {
                        Some(cheat) if !overflow => {
                            if cheat.codes.len() < MAX_CODES_PER_CHEAT {
                                cheat.codes.push(code);
                            }
                        }
                        Some(_) => {}
                        None => orphans += 1,
                    }
                }
                _ => {}
            }
        }
        if orphans > 0 {
            file.warnings.push(ChtWarning::CodesWithoutName(orphans));
        }
        if overflow {
            file.warnings.push(ChtWarning::TooManyCheats);
        }
        file
    }

    /// The pairs OPL hands to the cheat engine for the enabled cheats, and
    /// whether some had to be dropped.
    pub fn active_codes(&self, enabled: impl Fn(&Cheat) -> bool) -> (Vec<Code>, bool) {
        let all: Vec<Code> = self
            .cheats
            .iter()
            .filter(|c| c.is_master_code() || enabled(c))
            .flat_map(|c| c.effective_codes().iter().copied())
            .collect();
        let truncated = all.len() > MAX_ACTIVE_PAIRS;
        (all.into_iter().take(MAX_ACTIVE_PAIRS).collect(), truncated)
    }
}

/// `cheatConfigIsMasterCode()`.
pub fn is_master_code_name(name: &str) -> bool {
    let normalized = name
        .split_ascii_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "mastercode" | "master code" | "enable code (must be on)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"\"Grand Theft Auto III /ID SLUS_200.62\"\r\n\
Mastercode\r\n\
9029A2A8 0C0A6A1E\r\n\
\r\n\
Infinite Health // keep it\r\n\
2032D3B4 00000063\r\n\
2032D3B8 00000064\r\n\
# a comment\r\n\
Max Money\r\n\
2032D3BC0001869F\r\n";

    #[test]
    fn parses_names_and_codes() {
        let file = ChtFile::parse(SAMPLE);
        let names: Vec<&str> = file.cheats.iter().map(|c| c.name.as_str()).collect();
        // The quoted title line is just a name that "Mastercode" replaces.
        assert_eq!(names, ["Mastercode", "Infinite Health", "Max Money"]);
        assert!(file.cheats[0].is_master_code());
        assert_eq!(file.cheats[1].codes.len(), 2);
        assert_eq!(
            file.cheats[2].codes[0],
            Code {
                addr: 0x2032_D3BC,
                value: 0x0001_869F
            }
        );
        assert!(file.warnings.is_empty());
    }

    #[test]
    fn code_rules() {
        assert_eq!(
            parse_code(b"2032D3B4 00000063"),
            Some(Code {
                addr: 0x2032_D3B4,
                value: 0x63
            })
        );
        assert_eq!(
            parse_code(b"2032 D3B4\t0000 0063"),
            Some(Code {
                addr: 0x2032_D3B4,
                value: 0x63
            })
        );
        assert_eq!(parse_code(b"2032D3B4 0000006"), None);
        assert_eq!(parse_code(b"2032D3B4 000000634"), None);
        assert_eq!(parse_code(b"2032D3B4-00000063"), None);
    }

    #[test]
    fn orphan_codes_and_zero_codes() {
        let file =
            ChtFile::parse(b"2032D3B4 00000063\n00000000 00000000\nName\n00000000 00000000\n");
        assert!(file.cheats.is_empty());
        assert_eq!(file.warnings, [ChtWarning::CodesWithoutName(1)]);
    }

    #[test]
    fn indented_comment_becomes_blank_name() {
        let file = ChtFile::parse(b"Infinite Health\n  // the code below\n2032D3B4 00000063\n");
        assert_eq!(file.cheats.len(), 1);
        assert_eq!(file.cheats[0].name, "  ");
        assert_eq!(file.warnings, [ChtWarning::BlankName { line: 2 }]);
    }

    #[test]
    fn long_lines_wrap() {
        let mut text = "N".repeat(LINE_MAX);
        text.push('X');
        text.push_str("2032D3B4 00000063\n");
        let file = ChtFile::parse(text.as_bytes());
        // The 256th byte is lost and the tail is a code line of its own.
        assert_eq!(file.cheats.len(), 1);
        assert_eq!(file.cheats[0].name.len(), NAME_MAX);
        assert_eq!(file.cheats[0].codes.len(), 1);
    }

    #[test]
    fn cheat_limit() {
        let text: String = (0..=MAX_CHEATS)
            .map(|i| format!("C{i}\n2032D3B4 {i:08X}\n"))
            .collect();
        let file = ChtFile::parse(text.as_bytes());
        assert_eq!(file.cheats.len(), MAX_CHEATS);
        assert_eq!(file.warnings, [ChtWarning::TooManyCheats]);
    }

    #[test]
    fn active_code_budget() {
        let text: String = (0..200)
            .map(|i| format!("C{i}\n2032D3B4 {i:08X}\n10000000 00000001\n"))
            .collect();
        let file = ChtFile::parse(text.as_bytes());
        let (codes, truncated) = file.active_codes(|_| true);
        assert_eq!(codes.len(), MAX_ACTIVE_PAIRS);
        assert!(truncated);
        let (codes, truncated) = file.active_codes(|c| c.name == "C1");
        assert_eq!(codes.len(), 2);
        assert!(!truncated);
    }

    #[test]
    fn address_zero_ends_a_cheat() {
        let file = ChtFile::parse(b"A\n10000000 00000001\n00000000 00000005\n10000000 00000002\n");
        assert_eq!(file.cheats[0].codes.len(), 3);
        assert_eq!(file.cheats[0].effective_codes().len(), 1);
    }

    #[test]
    fn master_code_names() {
        for name in ["Mastercode", "  MASTER   code ", "Enable Code (Must Be On)"] {
            assert!(is_master_code_name(name), "{name}");
        }
        assert!(!is_master_code_name("Master Code 2"));
    }
}
