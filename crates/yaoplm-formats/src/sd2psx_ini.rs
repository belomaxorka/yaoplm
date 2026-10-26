//! INI files on a memory card emulator's SD: sd2psx and sd2psXtd (DEV-2,
//! ADR-0048).
//!
//! Reference: `docs/research/19-psxmemcard-gen2-audit.md` §2–§3 and
//! research/07 §2.7. Mirrors inih as sd2psXtd/firmware@8a20bd9 (tag 1.4.0)
//! builds it (`ext/inih/ini.c`, `ext/inih/ini.h`, `ini_reader_sd()`), and
//! the keys `src/settings.c` and `src/card_config.c` read. The files:
//! `.sd2psx/settings.ini`, `.sd2psx/Game2Folder.ini`, and next to each card
//! folder's images `<folder>/<folder>.ini` (`BOOT/BootCard.ini`,
//! `Card1/Card1.ini`).
//!
//! How the firmware reads a file:
//! - Lines are read 199 bytes at a time (`INI_MAX_LINE` 200): the rest of a
//!   longer line is read as the next line.
//! - **A last line without a line feed is never read**: `ini_reader_sd()`
//!   gives up at the end of the file in the middle of a line. That is the
//!   README's "the last line must be empty".
//! - A UTF-8 BOM on the first line is skipped; each line is trimmed.
//! - `;` or `#` first: a comment. `;` after whitespace inside a line starts
//!   an inline comment.
//! - `[section]`: the name up to `]`, as it is, at most 49 bytes.
//! - `name=value` or `name:value`, both trimmed. A line with leading
//!   whitespace after a name continues it: the firmware sees that name again
//!   with the line as its value.
//! - Sections and names are matched case-sensitively (`strcmp`); a later
//!   value replaces an earlier one. Numbers are read with `atoi`.

/// `INI_MAX_LINE - 1`: what one read of the firmware's reader returns.
const READ_MAX: usize = 199;
/// `MAX_SECTION - 1`.
const SECTION_MAX: usize = 49;
/// `MAX_NAME - 1`: the name a continuation line is reported under.
const NAME_MAX: usize = 49;

/// One `name=value` the firmware sees, in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub section: String,
    pub key: String,
    pub value: String,
    /// 1-based, counting the firmware's reads.
    pub line: usize,
}

/// A parsed file: what the firmware reads from it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Ini {
    pub entries: Vec<Entry>,
    /// Lines the firmware cannot read (no `=`, `:` or `]`), 1-based.
    pub errors: Vec<usize>,
    /// The last line, when it has no line feed: the firmware never reads it.
    pub unread_tail: Option<String>,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn trim(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|&b| !is_space(b))
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| !is_space(b))
        .map_or(start, |e| e + 1);
    &bytes[start..end.max(start)]
}

/// `ini_find_chars_or_comment()`: the first of `chars`, or an inline `;`
/// after whitespace, or the end.
fn find_or_comment(bytes: &[u8], chars: &[u8]) -> usize {
    let mut was_space = false;
    for (i, &b) in bytes.iter().enumerate() {
        if chars.contains(&b) || (was_space && b == b';') {
            return i;
        }
        was_space = is_space(b);
    }
    bytes.len()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `ini_reader_sd()`: reads of at most 199 bytes, each ending after a line
/// feed; the unterminated tail is returned apart.
fn reads(bytes: &[u8]) -> (Vec<&[u8]>, Option<&[u8]>) {
    let mut out = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let limit = rest.len().min(READ_MAX);
        match rest[..limit].iter().position(|&b| b == b'\n') {
            Some(lf) => {
                out.push(&rest[..=lf]);
                rest = &rest[lf + 1..];
            }
            None if limit == READ_MAX => {
                out.push(&rest[..limit]);
                rest = &rest[limit..];
            }
            None => return (out, Some(rest)),
        }
    }
    (out, None)
}

impl Ini {
    pub fn parse(bytes: &[u8]) -> Self {
        let (lines, tail) = reads(bytes);
        let mut ini = Self {
            unread_tail: tail.map(text).filter(|t| !t.trim().is_empty()),
            ..Self::default()
        };
        let mut section = String::new();
        let mut prev_name: Option<String> = None;
        for (index, raw) in lines.into_iter().enumerate() {
            let number = index + 1;
            // A C string: a NUL ends it.
            let raw = raw.split(|&b| b == 0).next().unwrap_or_default();
            let raw = if number == 1 {
                raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw)
            } else {
                raw
            };
            let indented = raw.first().is_some_and(|&b| is_space(b));
            let line = trim(raw);
            match line.first() {
                None | Some(b';' | b'#') => {}
                Some(_) if indented && prev_name.is_some() => {
                    let value = trim(&line[..find_or_comment(line, b"")]);
                    if let Some(name) = &prev_name {
                        ini.entries.push(Entry {
                            section: section.clone(),
                            key: name.clone(),
                            value: text(value),
                            line: number,
                        });
                    }
                }
                Some(b'[') => {
                    let body = &line[1..];
                    let end = find_or_comment(body, b"]");
                    if body.get(end) == Some(&b']') {
                        section = text(&body[..end.min(SECTION_MAX)]);
                        prev_name = None;
                    } else {
                        ini.errors.push(number);
                    }
                }
                Some(_) => {
                    let split = find_or_comment(line, b"=:");
                    if matches!(line.get(split), Some(b'=' | b':')) {
                        let raw_name = trim(&line[..split]);
                        let name = text(raw_name);
                        let value = &line[split + 1..];
                        let value = trim(&value[..find_or_comment(value, b"")]);
                        // inih keeps the name in 50 bytes and continues only
                        // after a non-empty one.
                        prev_name = (!raw_name.is_empty())
                            .then(|| text(&raw_name[..raw_name.len().min(NAME_MAX)]));
                        ini.entries.push(Entry {
                            section: section.clone(),
                            key: name,
                            value: text(value),
                            line: number,
                        });
                    } else {
                        ini.errors.push(number);
                    }
                }
            }
        }
        ini
    }

    /// The value the firmware ends up with: the last one.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.section == section && e.key == key)
            .map(|e| e.value.as_str())
    }

    /// Sections in file order, each with its entries as written.
    pub fn sections(&self) -> Vec<(String, Vec<(String, String)>)> {
        let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
        for entry in &self.entries {
            let pair = (entry.key.clone(), entry.value.clone());
            match out.iter_mut().find(|(name, _)| *name == entry.section) {
                Some((_, pairs)) => pairs.push(pair),
                None => out.push((entry.section.clone(), vec![pair])),
            }
        }
        out
    }
}

/// C's `atoi`: leading whitespace, a sign, then digits; anything else is 0.
pub fn atoi(value: &str) -> i64 {
    let bytes = value.as_bytes();
    let mut i = bytes.iter().take_while(|&&b| is_space(b)).count();
    let negative = match bytes.get(i) {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };
    let mut n: i64 = 0;
    for &b in &bytes[i..] {
        if !b.is_ascii_digit() {
            break;
        }
        n = n.saturating_mul(10).saturating_add(i64::from(b - b'0'));
    }
    if negative { -n } else { n }
}

/// A card size in MiB the firmware accepts (`settings.c`, `card_config.c`):
/// any other value leaves the previous one.
pub fn card_size(value: &str) -> Option<u32> {
    match atoi(value) {
        size @ (1 | 2 | 4 | 8 | 16 | 32 | 64 | 128) => u32::try_from(size).ok(),
        _ => None,
    }
}

/// The last value of `section`/`key` that `accept` takes: the firmware's
/// handlers ignore values they do not accept and keep the earlier one.
fn last_accepted<T>(
    ini: &Ini,
    section: &str,
    key: &str,
    accept: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    ini.entries
        .iter()
        .filter(|e| e.section == section && e.key == key)
        .filter_map(|e| accept(&e.value))
        .last()
}

/// The card size of `.sd2psx/settings.ini` (`[PS2] CardSize`), the size the
/// firmware creates new PS2 cards with.
pub fn settings_card_size(ini: &Ini) -> Option<u32> {
    last_accepted(ini, "PS2", "CardSize", card_size)
}

/// `MaxChannels`: 1 to 255.
fn max_channels(value: &str) -> Option<u32> {
    Some(atoi(value))
        .filter(|n| (1..=i64::from(u8::MAX)).contains(n))
        .and_then(|n| u32::try_from(n).ok())
}

/// What a card folder's ini says (`card_config.c`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CardIni {
    /// `[ChannelName]`: channel number → name, numbers as the firmware
    /// asks for them (decimal, no leading zeros).
    pub names: Vec<(u32, String)>,
    /// `[Settings] MaxChannels`; the firmware assumes 8 without it.
    pub max_channels: Option<u32>,
    /// `[Settings] CardSize`.
    pub card_size: Option<u32>,
}

impl CardIni {
    pub fn read(ini: &Ini) -> Self {
        let mut names: Vec<(u32, String)> = Vec::new();
        for entry in ini.entries.iter().filter(|e| e.section == "ChannelName") {
            // The firmware looks a channel up by its decimal string.
            let Ok(channel) = entry.key.parse::<u32>() else {
                continue;
            };
            if channel.to_string() != entry.key {
                continue;
            }
            match names.iter_mut().find(|(c, _)| *c == channel) {
                Some((_, name)) => name.clone_from(&entry.value),
                None => names.push((channel, entry.value.clone())),
            }
        }
        names.sort_by_key(|(c, _)| *c);
        Self {
            names,
            max_channels: last_accepted(ini, "Settings", "MaxChannels", max_channels),
            card_size: last_accepted(ini, "Settings", "CardSize", card_size),
        }
    }

    pub fn name(&self, channel: u32) -> Option<&str> {
        self.names
            .iter()
            .find(|(c, _)| *c == channel)
            .map(|(_, n)| n.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factory_settings_read_as_the_firmware_reads_them() {
        // research/19 §2: the factory card's settings.ini, LF.
        let ini = Ini::parse(
            b"[General]\nMode=PS2\nFlippedScreen=OFF\n[PS1]\nAutoboot=OFF\nGameID=ON\n\
              [PS2]\nAutoboot=ON\nGameID=ON\nCardSize=64\nVariant=RETAIL\n",
        );
        assert_eq!(ini.get("PS2", "CardSize"), Some("64"));
        assert_eq!(settings_card_size(&ini), Some(64));
        assert_eq!(ini.get("General", "Mode"), Some("PS2"));
        assert!(ini.errors.is_empty() && ini.unread_tail.is_none());
        let sections = ini.sections();
        assert_eq!(
            sections.iter().map(|(s, _)| s.as_str()).collect::<Vec<_>>(),
            ["General", "PS1", "PS2"]
        );
        assert_eq!(sections[2].1.len(), 4);
    }

    #[test]
    fn a_last_line_without_a_line_feed_is_lost() {
        let ini = Ini::parse(b"[PS2]\nAutoboot=ON\nCardSize=8");
        assert_eq!(ini.get("PS2", "CardSize"), None);
        assert_eq!(ini.unread_tail.as_deref(), Some("CardSize=8"));
        // An empty last line, or a CR LF file, loses nothing.
        let ini = Ini::parse(b"[PS2]\r\nCardSize=8\r\n");
        assert_eq!(settings_card_size(&ini), Some(8));
        assert!(ini.unread_tail.is_none());
        assert!(Ini::parse(b"[PS2]\nCardSize=8\n   ").unread_tail.is_none());
    }

    #[test]
    fn names_and_sections_are_case_sensitive_and_the_last_value_wins() {
        let ini = Ini::parse(b"[ps2]\nCardSize=16\n[PS2]\ncardsize=32\nCardSize=8\nCardSize=4\n");
        assert_eq!(settings_card_size(&ini), Some(4));
        assert_eq!(ini.get("PS2", "cardsize"), Some("32"));
        assert_eq!(ini.get("ps2", "CardSize"), Some("16"));
    }

    #[test]
    fn inih_syntax() {
        let ini = Ini::parse(
            "\u{feff}; comment\n# another\n[ChannelName]\n1 = FMCB 1.966 ; inline\n2:FunTuna;not a comment\n\
             3=Name\n  continued\n[Broken\nno separator\n[Settings]\nMaxChannels=9\n"
                .as_bytes(),
        );
        let card = CardIni::read(&ini);
        assert_eq!(card.name(1), Some("FMCB 1.966"));
        assert_eq!(card.name(2), Some("FunTuna;not a comment"));
        // A continuation replaces the value the firmware keeps.
        assert_eq!(card.name(3), Some("continued"));
        assert_eq!(card.max_channels, Some(9));
        assert_eq!(ini.errors, [8, 9]);
        // `[Broken` leaves the section as it was.
        assert_eq!(
            ini.entries.last().map(|e| e.section.as_str()),
            Some("Settings")
        );
    }

    #[test]
    fn long_lines_go_on_as_the_next_read() {
        let mut file = b"[ChannelName]\n1=".to_vec();
        file.extend(std::iter::repeat_n(b'x', 250));
        file.extend(b"\n2=two\n");
        let ini = Ini::parse(&file);
        let card = CardIni::read(&ini);
        // The first read holds "1=" and 197 x; the rest has no separator.
        assert_eq!(card.name(1).map(str::len), Some(197));
        assert_eq!(card.name(2), Some("two"));
        assert_eq!(ini.errors, [3]);
    }

    #[test]
    fn channel_numbers_are_decimal_strings() {
        let ini = Ini::parse(b"[ChannelName]\n01=zero-padded\n1=one\n+2=plus\n10=ten\n");
        let card = CardIni::read(&ini);
        assert_eq!(card.names, [(1, "one".into()), (10, "ten".into())]);
    }

    #[test]
    fn sizes_follow_atoi_and_the_firmware_s_list() {
        assert_eq!(card_size("8"), Some(8));
        assert_eq!(card_size(" 16MB"), Some(16));
        assert_eq!(card_size("+128"), Some(128));
        for bad in ["0", "3", "256", "-8", "eight", ""] {
            assert_eq!(card_size(bad), None, "{bad}");
        }
        assert_eq!(atoi("99999999999999999999999"), i64::MAX);
        let ini = Ini::parse(b"[Settings]\nMaxChannels=0\nCardSize=12\n");
        let card = CardIni::read(&ini);
        assert_eq!((card.max_channels, card.card_size), (None, None));
        let ini = Ini::parse(b"[Settings]\nMaxChannels=255\n");
        assert_eq!(CardIni::read(&ini).max_channels, Some(255));
        let ini = Ini::parse(b"[Settings]\nMaxChannels=256\n");
        assert_eq!(CardIni::read(&ini).max_channels, None);
    }

    #[test]
    fn a_bad_repeat_keeps_the_last_good_value() {
        // The firmware's handlers only take values they accept, so a later
        // bad one leaves the earlier good one (`settings.c`, `card_config.c`).
        let ini = Ini::parse(b"[PS2]\nCardSize=64\nCardSize=abc\nCardSize=3\n");
        assert_eq!(settings_card_size(&ini), Some(64));
        let ini =
            Ini::parse(b"[Settings]\nMaxChannels=4\nMaxChannels=0\nCardSize=8\nCardSize=12\n");
        let card = CardIni::read(&ini);
        assert_eq!((card.max_channels, card.card_size), (Some(4), Some(8)));
    }

    #[test]
    fn an_empty_name_starts_no_continuation() {
        // inih continues only after a non-empty name (`*prev_name`).
        let ini = Ini::parse(b"[ChannelName]\n=nameless\n  1=after\n");
        assert_eq!(CardIni::read(&ini).name(1), Some("after"));
        // A long name continues under its first 49 bytes, as inih keeps it.
        let long = "k".repeat(60);
        let ini = Ini::parse(format!("[S]\n{long}=a\n  b\n").as_bytes());
        assert_eq!(ini.entries[1].key, "k".repeat(49));
    }

    #[test]
    fn anything_parses_without_a_panic() {
        for bytes in [
            &b""[..],
            b"\0\0\0",
            b"[",
            b"=",
            b"\xff\xfe[\xc3]\n\xc3=\xff\n",
            b"   \n  \n",
        ] {
            let _ = CardIni::read(&Ini::parse(bytes));
        }
    }
}
