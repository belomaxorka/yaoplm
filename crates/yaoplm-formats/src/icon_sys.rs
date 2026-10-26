//! `icon.sys`: what the console's browser shows for a save folder on a PS2
//! memory card, its title in two lines (VMC-2, ADR-0048).
//!
//! Format reference: `docs/research/19-psxmemcard-gen2-audit.md` §4 (the
//! titles that tell FreeMcBoot from PS2BBL on a boot card). Mirrors the
//! 964-byte `IconSys` that mymcplusplus reads (`mymcplusplus/ps2iconsys.py`,
//! GPL-3, Adubbz/mymcplusplus@3247c9c): magic `PS2D`, at 0x06 the byte
//! offset where the title's second line starts, background and light data,
//! the title at 0xC0 (68 bytes of Shift-JIS, NUL-padded), then the names of
//! the normal, copy and delete icons (64 bytes each).
//!
//! Titles mostly use full-width Latin letters (`ＦＲＥＥ　ＭＣＢＯＯＴ`);
//! [`IconSys::title`] gives them as plain ASCII for display and search.

const MAGIC: &[u8; 4] = b"PS2D";
pub const SIZE: usize = 964;
const TITLE_AT: usize = 0xC0;
const TITLE_LEN: usize = 68;
const ICON_AT: usize = TITLE_AT + TITLE_LEN;
const ICON_LEN: usize = 64;

/// A parsed `icon.sys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconSys {
    /// The title's two lines, decoded and with full-width Latin letters
    /// made plain. The second may be empty.
    pub title: [String; 2],
    /// The normal, copy and delete icon files.
    pub icons: [String; 3],
}

/// Shift-JIS to text; full-width ASCII and the ideographic space become
/// their plain forms.
pub fn decode_title(bytes: &[u8]) -> String {
    let (text, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    text.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFF01 + 0x21).unwrap_or(c),
            c => c,
        })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn until_nul(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    &bytes[..end]
}

impl IconSys {
    /// `None` when `bytes` is not an `icon.sys`.
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < SIZE || !bytes.starts_with(MAGIC) {
            return None;
        }
        let raw = until_nul(&bytes[TITLE_AT..TITLE_AT + TITLE_LEN]);
        // The break is a byte offset into the title; one that is odd, past
        // the end or inside a two-byte character still splits safely.
        let split = usize::from(u16::from_le_bytes([bytes[6], bytes[7]])).min(raw.len());
        let (first, second) = raw.split_at(split);
        let icon = |n: usize| {
            let at = ICON_AT + n * ICON_LEN;
            String::from_utf8_lossy(until_nul(&bytes[at..at + ICON_LEN])).into_owned()
        };
        Some(Self {
            title: [decode_title(first), decode_title(second)],
            icons: [icon(0), icon(1), icon(2)],
        })
    }

    /// Both lines in one, for display and search.
    pub fn title(&self) -> String {
        match (self.title[0].is_empty(), self.title[1].is_empty()) {
            (_, true) => self.title[0].clone(),
            (true, false) => self.title[1].clone(),
            (false, false) => format!("{} {}", self.title[0], self.title[1]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon_sys(title: &[u8], split: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; SIZE];
        bytes[..4].copy_from_slice(MAGIC);
        bytes[6..8].copy_from_slice(&split.to_le_bytes());
        bytes[TITLE_AT..TITLE_AT + title.len()].copy_from_slice(title);
        bytes[ICON_AT..ICON_AT + 8].copy_from_slice(b"list.ico");
        bytes
    }

    #[test]
    fn full_width_titles_read_as_plain_text() {
        // "ＦＲＥＥ　ＭＣＢＯＯＴ" in Shift-JIS, broken after "ＦＲＥＥ".
        let title = encoding_rs::SHIFT_JIS
            .encode("ＦＲＥＥ　ＭＣＢＯＯＴ")
            .0
            .into_owned();
        let parsed = IconSys::parse(&icon_sys(&title, 8)).unwrap();
        assert_eq!(parsed.title, ["FREE".to_owned(), "MCBOOT".to_owned()]);
        assert_eq!(parsed.title(), "FREE MCBOOT");
        assert_eq!(parsed.icons[0], "list.ico");
        // Kana stay as they are.
        let kana = encoding_rs::SHIFT_JIS.encode("セーブデータ").0.into_owned();
        assert_eq!(
            IconSys::parse(&icon_sys(&kana, 0)).unwrap().title(),
            "セーブデータ"
        );
    }

    #[test]
    fn odd_breaks_and_junk_do_not_panic() {
        let title = encoding_rs::SHIFT_JIS.encode("ＡＢ").0.into_owned();
        for split in [0u16, 1, 3, 200, u16::MAX] {
            let parsed = IconSys::parse(&icon_sys(&title, split)).unwrap();
            assert!(!parsed.title().is_empty(), "{split}");
        }
        assert_eq!(IconSys::parse(b"PS2D"), None);
        assert_eq!(IconSys::parse(&[0u8; SIZE]), None);
        let mut junk = vec![0xFFu8; SIZE];
        junk[..4].copy_from_slice(MAGIC);
        let _ = IconSys::parse(&junk);
    }
}
