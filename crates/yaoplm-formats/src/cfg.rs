//! OPL text configs (`CFG/<ID>.cfg`, `conf_*.cfg`, `title.cfg`) (CFG-1).
//!
//! Format reference: `docs/research/02-opl-formats.md` §6.1. Mirrors
//! `configReadFileBuffer()`, `splitAssignment()`, `parsePrefix()`,
//! `configSetStr()` and `configWrite()` in `src/config.c` and
//! `openFileBuffer()`/`readFileBuffer()` in `src/util.c` of
//! ps2homebrew/Open-PS2-Loader@b3a21997.
//!
//! How OPL reads a file:
//! - A UTF-8 BOM at the start is skipped. Lines end at LF; one CR before it
//!   (or at the end of the file) is dropped.
//! - A line whose first byte is `#` is a comment and skipped entirely.
//! - Otherwise spaces and tabs are skipped and the first `=` splits key and
//!   value. Nothing is trimmed: `key = v` stores the key `"key "` and the value
//!   `" v"`. Keys keep at most 31 bytes, values 255.
//! - A line without `=` but with `:` past column 0 sets a key prefix (themes):
//!   following indented lines become `prefix_key`; a non-indented `key=value`
//!   line clears it.
//! - A repeated key keeps its first position and takes the last value.
//!
//! How OPL writes one: every key except those starting with `#` (runtime
//! values such as `#Name`) as `key=value\r\n`, in list order.
//!
//! [`CfgDocument`] keeps every byte of the original file, so an unmodified
//! document serialises identically and edits leave unknown keys, their
//! order, comments and odd lines alone. New lines use CRLF.
//!
//! wOPL (since 2026-06-20, ps2homebrew/wOPL@03758fb) writes `CFG/<ID>.cfg` in
//! libconfig syntax instead; [`detect_dialect`] recognises those files so
//! callers can leave them alone.

/// `CONFIG_KEY_NAME_LEN - 1`.
pub const KEY_MAX: usize = 31;
/// `CONFIG_KEY_VALUE_LEN - 1`.
pub const VALUE_MAX: usize = 255;

const BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CfgError {
    #[error("key is empty")]
    EmptyKey,
    #[error("key {0:?} is longer than {KEY_MAX} bytes")]
    KeyTooLong(String),
    #[error("key {0:?} contains a character OPL cannot store")]
    BadKey(String),
    #[error("key {0:?} starts with '#': OPL never saves runtime keys")]
    RuntimeKey(String),
    #[error("value for {key:?} is {len} bytes; OPL keeps {VALUE_MAX}")]
    ValueTooLong { key: String, len: usize },
    #[error("value for {0:?} contains a line break or NUL")]
    BadValue(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `key=value`: `key_start` skips leading blanks, `eq` is the first `=`.
    Entry { key_start: usize, eq: usize },
    /// `#` in column 0.
    Comment,
    /// `name:` sets a prefix.
    Prefix { colon: usize },
    /// Blank or malformed: OPL logs and ignores it.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Line {
    /// Line content without its LF, but with a CR if the file had one.
    raw: Vec<u8>,
    /// Followed by LF in the file.
    lf: bool,
    kind: Kind,
}

impl Line {
    fn new(raw: Vec<u8>, lf: bool) -> Self {
        let kind = classify_line(content(&raw));
        Self { raw, lf, kind }
    }

    fn has_cr(&self) -> bool {
        self.raw.last() == Some(&b'\r')
    }
}

/// The line as OPL sees it: without the trailing CR.
fn content(raw: &[u8]) -> &[u8] {
    raw.strip_suffix(b"\r").unwrap_or(raw)
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn classify_line(line: &[u8]) -> Kind {
    if line.first() == Some(&b'#') {
        return Kind::Comment;
    }
    let key_start = line
        .iter()
        .position(|&b| !is_blank(b))
        .unwrap_or(line.len());
    if let Some(offset) = line[key_start..].iter().position(|&b| b == b'=') {
        return Kind::Entry {
            key_start,
            eq: key_start + offset,
        };
    }
    match line.iter().position(|&b| b == b':') {
        Some(colon) if colon > 0 => Kind::Prefix { colon },
        _ => Kind::Other,
    }
}

fn truncate(bytes: &[u8], max: usize) -> &[u8] {
    &bytes[..bytes.len().min(max)]
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// One key as OPL ends up storing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfgEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone)]
struct Resolved {
    key: Vec<u8>,
    value: Vec<u8>,
    /// Lines that assign this key, in file order.
    lines: Vec<usize>,
}

/// A config file, byte for byte, with OPL's view of its keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CfgDocument {
    bom: bool,
    lines: Vec<Line>,
}

impl CfgDocument {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn parse(bytes: &[u8]) -> Self {
        let (bom, mut rest) = match bytes.strip_prefix(BOM) {
            Some(rest) => (true, rest),
            None => (false, bytes),
        };
        let mut lines = Vec::new();
        while !rest.is_empty() {
            match rest.iter().position(|&b| b == b'\n') {
                Some(lf) => {
                    lines.push(Line::new(rest[..lf].to_vec(), true));
                    rest = &rest[lf + 1..];
                }
                None => {
                    lines.push(Line::new(rest.to_vec(), false));
                    rest = &[];
                }
            }
        }
        Self { bom, lines }
    }

    /// The original bytes, with edits applied.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.bom {
            out.extend_from_slice(BOM);
        }
        for line in &self.lines {
            out.extend_from_slice(&line.raw);
            if line.lf {
                out.push(b'\n');
            }
        }
        out
    }

    /// The file `configWrite()` would produce from this document.
    pub fn to_opl_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for entry in self.resolve() {
            if entry.key.first().is_some_and(|&b| b != b'#') {
                out.extend_from_slice(&entry.key);
                out.push(b'=');
                out.extend_from_slice(&entry.value);
                out.extend_from_slice(b"\r\n");
            }
        }
        out
    }

    /// Keys and values in OPL's order, as OPL would load them.
    pub fn entries(&self) -> Vec<CfgEntry> {
        self.resolve()
            .into_iter()
            .map(|entry| CfgEntry {
                key: lossy(&entry.key),
                value: lossy(&entry.value),
            })
            .collect()
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.find(key.as_bytes()).map(|entry| lossy(&entry.value))
    }

    pub fn get_int(&self, key: &str) -> Option<i32> {
        self.get(key).map(|value| atoi(&value))
    }

    /// Sets `key` the way OPL would: in place if it exists, appended
    /// otherwise. Later duplicates are removed so the new value wins.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), CfgError> {
        validate_key(key)?;
        validate_value(key, value)?;
        let Some(entry) = self.find(key.as_bytes()) else {
            self.push_line(format!("{key}={value}").into_bytes());
            return Ok(());
        };
        let (&first, duplicates) = entry.lines.split_first().unwrap_or((&usize::MAX, &[]));
        if let Some(line) = self.lines.get_mut(first)
            && let Kind::Entry { eq, .. } = line.kind
        {
            let cr = line.has_cr();
            line.raw.truncate(eq + 1);
            line.raw.extend_from_slice(value.as_bytes());
            if cr {
                line.raw.push(b'\r');
            }
        }
        self.remove_lines(duplicates);
        Ok(())
    }

    pub fn set_int(&mut self, key: &str, value: i32) -> Result<(), CfgError> {
        self.set(key, &value.to_string())
    }

    /// Removes every line that assigns `key`. Returns whether any did.
    pub fn remove(&mut self, key: &str) -> bool {
        match self.find(key.as_bytes()) {
            Some(entry) => {
                self.remove_lines(&entry.lines);
                true
            }
            None => false,
        }
    }

    fn remove_lines(&mut self, indices: &[usize]) {
        let mut index = 0;
        self.lines.retain(|_| {
            let keep = !indices.contains(&index);
            index += 1;
            keep
        });
    }

    fn push_line(&mut self, mut raw: Vec<u8>) {
        if let Some(last) = self.lines.last_mut()
            && !last.lf
        {
            if !last.has_cr() {
                last.raw.push(b'\r');
            }
            last.lf = true;
        }
        raw.push(b'\r');
        self.lines.push(Line::new(raw, true));
    }

    fn find(&self, key: &[u8]) -> Option<Resolved> {
        let key = truncate(key, KEY_MAX);
        self.resolve().into_iter().find(|entry| entry.key == key)
    }

    /// Replays `configReadFileBuffer()`.
    fn resolve(&self) -> Vec<Resolved> {
        let mut entries: Vec<Resolved> = Vec::new();
        let mut prefix: Vec<u8> = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let text = content(&line.raw);
            match line.kind {
                Kind::Entry { key_start, eq } => {
                    if !text.first().is_some_and(|&b| is_blank(b)) {
                        prefix.clear();
                    }
                    let local = truncate(&text[key_start..eq], KEY_MAX + 1);
                    let key = if prefix.is_empty() {
                        truncate(local, KEY_MAX).to_vec()
                    } else {
                        let mut composed = prefix.clone();
                        composed.push(b'_');
                        composed.extend_from_slice(local);
                        composed.truncate(KEY_MAX);
                        composed
                    };
                    if key.is_empty() {
                        continue;
                    }
                    let value = truncate(&text[eq + 1..], VALUE_MAX).to_vec();
                    match entries.iter_mut().find(|e| e.key == key) {
                        Some(existing) => {
                            existing.value = value;
                            existing.lines.push(index);
                        }
                        None => entries.push(Resolved {
                            key,
                            value,
                            lines: vec![index],
                        }),
                    }
                }
                Kind::Prefix { colon } => prefix = truncate(&text[..colon], KEY_MAX).to_vec(),
                Kind::Comment | Kind::Other => {}
            }
        }
        entries
    }
}

/// C `atoi`: optional blanks and sign, then digits; anything else stops it.
fn atoi(value: &str) -> i32 {
    let trimmed = value.trim_start_matches([' ', '\t', '\n', '\r', '\x0B', '\x0C']);
    let (negative, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (true, &trimmed[1..]),
        Some(b'+') => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    // Out-of-range values are not portable in C either; saturate.
    let magnitude = digits
        .bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0i64, |acc, d| (acc * 10 + i64::from(d - b'0')).min(1 << 32));
    let signed = if negative { -magnitude } else { magnitude };
    i32::try_from(signed).unwrap_or(if negative { i32::MIN } else { i32::MAX })
}

fn validate_key(key: &str) -> Result<(), CfgError> {
    if key.is_empty() {
        return Err(CfgError::EmptyKey);
    }
    if key.starts_with('#') {
        return Err(CfgError::RuntimeKey(key.to_owned()));
    }
    if key.len() > KEY_MAX {
        return Err(CfgError::KeyTooLong(key.to_owned()));
    }
    // OPL would strip leading blanks and split at the first '='.
    let bad = key.starts_with([' ', '\t']) || key.contains(['=', '\r', '\n', '\0']);
    if bad {
        return Err(CfgError::BadKey(key.to_owned()));
    }
    Ok(())
}

fn validate_value(key: &str, value: &str) -> Result<(), CfgError> {
    if value.len() > VALUE_MAX {
        return Err(CfgError::ValueTooLong {
            key: key.to_owned(),
            len: value.len(),
        });
    }
    if value.contains(['\r', '\n', '\0']) {
        return Err(CfgError::BadValue(key.to_owned()));
    }
    Ok(())
}

/// Which loader's syntax a `.cfg` file uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// OPL `key=value`.
    Opl,
    /// libconfig, as written by wOPL. Not ours to rewrite.
    Libconfig,
}

/// Recognises libconfig files: group braces, or settings that all look like
/// `name = <literal>;`.
pub fn detect_dialect(bytes: &[u8]) -> Dialect {
    let text = String::from_utf8_lossy(bytes.strip_prefix(BOM).unwrap_or(bytes));
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with("//"))
        .collect();
    let braces = lines
        .iter()
        .any(|line| *line == "{" || line.starts_with('}') || line.ends_with('{'));
    if braces || (!lines.is_empty() && lines.iter().all(|line| is_libconfig_setting(line))) {
        Dialect::Libconfig
    } else {
        Dialect::Opl
    }
}

fn is_libconfig_setting(line: &str) -> bool {
    let Some(body) = line.strip_suffix(';') else {
        return false;
    };
    let Some(split) = body.find(['=', ':']) else {
        return false;
    };
    let (name, value) = (body[..split].trim(), body[split + 1..].trim());
    let name_ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '*')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '*'));
    let value_ok = (value.len() >= 2 && value.starts_with('"') && value.ends_with('"'))
        || value.starts_with(['[', '('])
        || matches!(value.to_ascii_lowercase().as_str(), "true" | "false")
        || (!value.is_empty()
            && value
                .chars()
                .all(|c| c.is_ascii_hexdigit() || matches!(c, '-' | '+' | '.' | 'x' | 'X' | 'L')));
    name_ok && value_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(doc: &CfgDocument) -> Vec<(String, String)> {
        doc.entries()
            .into_iter()
            .map(|e| (e.key, e.value))
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn reads_like_opl() {
        let doc = CfgDocument::parse(b"$Compatibility=5\r\nTitle=Ico\r\n$VMC_0=generic_0\r\n");
        assert_eq!(
            entries(&doc),
            pairs(&[
                ("$Compatibility", "5"),
                ("Title", "Ico"),
                ("$VMC_0", "generic_0")
            ])
        );
        assert_eq!(doc.get_int("$Compatibility"), Some(5));
        assert_eq!(doc.get("Missing"), None);
    }

    #[test]
    fn nothing_is_trimmed() {
        let doc = CfgDocument::parse(b"  Title = Ico \nkey==v\n");
        assert_eq!(entries(&doc), pairs(&[("Title ", " Ico "), ("key", "=v")]));
        assert_eq!(doc.get("Title"), None);
    }

    #[test]
    fn comments_bom_and_malformed_lines() {
        let doc = CfgDocument::parse(b"\xEF\xBB\xBF#Name=x\n\nno equals\n=orphan\n # a=b\nk=v");
        // Column-0 '#' lines are skipped; an indented '#' key is a key.
        assert_eq!(entries(&doc), pairs(&[("# a", "b"), ("k", "v")]));
        // OPL never writes '#' keys.
        assert_eq!(doc.to_opl_bytes(), b"k=v\r\n");
    }

    #[test]
    fn duplicates_keep_first_position_and_last_value() {
        let doc = CfgDocument::parse(b"a=1\nb=2\na=3\n");
        assert_eq!(entries(&doc), pairs(&[("a", "3"), ("b", "2")]));
    }

    #[test]
    fn length_limits() {
        let long_key = "K".repeat(40);
        let long_value = "V".repeat(300);
        let doc = CfgDocument::parse(format!("{long_key}={long_value}\n").as_bytes());
        let e = &doc.entries()[0];
        assert_eq!(e.key.len(), KEY_MAX);
        assert_eq!(e.value.len(), VALUE_MAX);
    }

    #[test]
    fn prefixes() {
        let doc =
            CfgDocument::parse(b"main0:\n\ttype=Background\n\tcolor=#000000\nother=1\n\tx=2\n");
        assert_eq!(
            entries(&doc),
            pairs(&[
                ("main0_type", "Background"),
                ("main0_color", "#000000"),
                ("other", "1"),
                ("x", "2"),
            ])
        );
        // A colon at column 0 is no prefix.
        let doc = CfgDocument::parse(b":x\n\ta=1\n");
        assert_eq!(entries(&doc), pairs(&[("a", "1")]));
    }

    #[test]
    fn cr_handling() {
        // Only the CR right before LF (or at EOF) goes.
        let doc = CfgDocument::parse(b"a=1\r\nb=2\r\r\nc=3\r");
        assert_eq!(
            entries(&doc),
            pairs(&[("a", "1"), ("b", "2\r"), ("c", "3")])
        );
        // Old Mac files are one long line.
        let doc = CfgDocument::parse(b"a=1\rb=2\r");
        assert_eq!(entries(&doc), pairs(&[("a", "1\rb=2")]));
    }

    #[test]
    fn unmodified_round_trip_is_byte_exact() {
        let inputs: [&[u8]; 5] = [
            b"",
            b"a=1",
            b"\xEF\xBB\xBF#c\r\n a = b \n\n\xFF=\xFE\r\nx:\n\ty=z",
            b"$Compatibility=5\r\nTitle=Ico\r\n",
            b"\r\n\r\n",
        ];
        for input in inputs {
            assert_eq!(CfgDocument::parse(input).to_bytes(), input);
        }
    }

    #[test]
    fn set_preserves_unknown_keys_and_order() {
        let mut doc = CfgDocument::parse(b"Unknown=1\r\n$Compatibility=1\r\nZzz=2\r\n");
        doc.set("$Compatibility", "3").unwrap();
        doc.set("$VMC_0", "card").unwrap();
        assert_eq!(
            doc.to_bytes(),
            b"Unknown=1\r\n$Compatibility=3\r\nZzz=2\r\n$VMC_0=card\r\n"
        );
    }

    #[test]
    fn set_keeps_line_style_and_drops_duplicates() {
        let mut doc = CfgDocument::parse(b"a=1\nb=2\na=3");
        doc.set("a", "9").unwrap();
        assert_eq!(doc.to_bytes(), b"a=9\nb=2\n");
        assert_eq!(doc.get("a").as_deref(), Some("9"));

        let mut doc = CfgDocument::parse(b"a=1");
        doc.set("b", "2").unwrap();
        assert_eq!(doc.to_bytes(), b"a=1\r\nb=2\r\n");
    }

    #[test]
    fn set_inside_prefix_section_keeps_local_key() {
        let mut doc = CfgDocument::parse(b"main0:\n\ttype=A\n");
        doc.set("main0_type", "B").unwrap();
        assert_eq!(doc.to_bytes(), b"main0:\n\ttype=B\n");
    }

    #[test]
    fn remove() {
        let mut doc = CfgDocument::parse(b"a=1\r\nb=2\r\na=3\r\n");
        assert!(doc.remove("a"));
        assert!(!doc.remove("a"));
        assert_eq!(doc.to_bytes(), b"b=2\r\n");
    }

    #[test]
    fn validation() {
        let mut doc = CfgDocument::new();
        assert_eq!(doc.set("", "x"), Err(CfgError::EmptyKey));
        assert!(matches!(
            doc.set("#Name", "x"),
            Err(CfgError::RuntimeKey(_))
        ));
        assert!(matches!(
            doc.set(&"k".repeat(32), "x"),
            Err(CfgError::KeyTooLong(_))
        ));
        assert!(matches!(doc.set("a=b", "x"), Err(CfgError::BadKey(_))));
        assert!(matches!(doc.set(" a", "x"), Err(CfgError::BadKey(_))));
        assert!(matches!(
            doc.set("a", &"v".repeat(256)),
            Err(CfgError::ValueTooLong { .. })
        ));
        assert!(matches!(doc.set("a", "x\ny"), Err(CfgError::BadValue(_))));
        assert!(doc.set(&"k".repeat(31), &"v".repeat(255)).is_ok());
        // Values may contain '=' and '#'.
        assert!(doc.set("Description", "a=b #1").is_ok());
        assert_eq!(doc.get("Description").as_deref(), Some("a=b #1"));
    }

    #[test]
    fn opl_writer_output() {
        let doc = CfgDocument::parse(b"#Size=700\nb=2\nmain0:\n\tt=x\n");
        assert_eq!(doc.to_opl_bytes(), b"b=2\r\nmain0_t=x\r\n");
    }

    #[test]
    fn atoi_matches_c() {
        for (input, expected) in [
            ("5", 5),
            (" -12x", -12),
            ("+7", 7),
            ("abc", 0),
            ("", 0),
            ("0x10", 0),
        ] {
            assert_eq!(atoi(input), expected, "{input:?}");
        }
    }

    #[test]
    fn dialects() {
        assert_eq!(
            detect_dialect(b"$Compatibility=5\r\nTitle=Ico\r\n"),
            Dialect::Opl
        );
        assert_eq!(detect_dialect(b""), Dialect::Opl);
        assert_eq!(
            detect_dialect(b"compat :\n{\n  mode = 3;\n};\n"),
            Dialect::Libconfig
        );
        assert_eq!(
            detect_dialect(b"title = \"Ico\";\nvmc0 = \"generic_0\";\nmode = 3;\n"),
            Dialect::Libconfig
        );
        // One OPL-style line is enough to stay OPL.
        assert_eq!(
            detect_dialect(b"title = \"Ico\";\nTitle=Ico\n"),
            Dialect::Opl
        );
        assert_eq!(detect_dialect(b"Description=Ends with;\n"), Dialect::Opl);
    }
}
