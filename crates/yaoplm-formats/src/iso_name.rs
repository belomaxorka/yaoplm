//! Image file names in `CD/` and `DVD/` (IMP-4).
//!
//! Format reference: `docs/research/02-opl-formats.md` §4.2. Mirrors
//! `isValidIsoName()` and the name handling in `scanForISO()`
//! (`src/supportbase.c`, ps2homebrew/Open-PS2-Loader@b3a21997), plus the SMB
//! file-name buffer `cdvdman_settings_smb.filename[160]`
//! (`modules/iopcore/common/cdvd_config.h`).
//!
//! - Only `.iso` and `.zso` are listed, compared case-insensitively.
//! - "Old" names `SLUS_200.62.Title.iso` (length >= 17, `_` at 4, `.` at 8
//!   and 11) carry the ID; OPL then skips mounting the image. Any name that
//!   happens to match the pattern is read this way.
//! - Other names `Title.iso` are mounted and the ID comes from `SYSTEM.CNF`.
//! - The title may have at most 160 characters, otherwise OPL skips the file.
//! - The in-game SMB driver keeps the file name in 160 bytes with a
//!   terminator, so SMB names must stay within 159 bytes.
//!
//! OPL counts characters of the FatFs OEM name (code page 869), one byte per
//! character, so lengths here are counted in `char`s. Non-ASCII names are
//! allowed but get mangled on the console; writers should avoid them.

use std::fmt;

/// `ISO_GAME_NAME_MAX`.
pub const TITLE_MAX: usize = 160;
/// Longest image file name the SMB driver can open.
pub const SMB_FILE_NAME_MAX: usize = 159;
/// Shortest old-style name: 11-char ID, `.`, one title char, extension.
const OLD_FORMAT_MIN_LEN: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageExtension {
    Iso,
    Zso,
}

impl ImageExtension {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Iso => ".iso",
            Self::Zso => ".zso",
        }
    }
}

impl fmt::Display for ImageExtension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `GAME_FORMAT_OLD_ISO` / `GAME_FORMAT_ISO`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NameFormat {
    /// `ID.Title.ext`: the ID comes from the name.
    Old,
    /// `Title.ext`: the ID comes from `SYSTEM.CNF`.
    New,
}

/// An image name as OPL splits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoFileName {
    pub format: NameFormat,
    /// For [`NameFormat::Old`], the 11 characters before the title.
    pub startup: Option<String>,
    pub title: String,
    pub extension: ImageExtension,
    /// The extension exactly as written. OPL lists `.ISO`, but only shows the
    /// ISO/ZSO format label for lowercase extensions.
    pub raw_extension: String,
}

impl IsoFileName {
    pub fn lowercase_extension(&self) -> bool {
        self.raw_extension == self.extension.as_str()
    }
}

/// Why OPL does not list a file in `CD/` or `DVD/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameSkip {
    #[error("not an .iso or .zso file")]
    NotAnImage,
    #[error("title is longer than {TITLE_MAX} characters")]
    TitleTooLong,
}

/// Splits `name` like `isValidIsoName()` and applies the title limit from
/// `scanForISO()`.
pub fn classify(name: &str) -> Result<IsoFileName, NameSkip> {
    let chars: Vec<char> = name.chars().collect();
    let len = chars.len();
    // OPL needs at least ".iso" plus one character.
    if len < 5 {
        return Err(NameSkip::NotAnImage);
    }
    let raw_extension: String = chars[len - 4..].iter().collect();
    let extension = match raw_extension.to_ascii_lowercase().as_str() {
        ".iso" => ImageExtension::Iso,
        ".zso" => ImageExtension::Zso,
        _ => return Err(NameSkip::NotAnImage),
    };
    let old = len >= OLD_FORMAT_MIN_LEN && chars[4] == '_' && chars[8] == '.' && chars[11] == '.';
    let (format, startup, title) = if old {
        (
            NameFormat::Old,
            Some(chars[..11].iter().collect()),
            chars[12..len - 4].iter().collect::<String>(),
        )
    } else {
        (NameFormat::New, None, chars[..len - 4].iter().collect())
    };
    if title.chars().count() > TITLE_MAX {
        return Err(NameSkip::TitleTooLong);
    }
    Ok(IsoFileName {
        format,
        startup,
        title,
        extension,
        raw_extension,
    })
}

/// True when `id` can lead an old-style name, i.e. OPL would read it back
/// from the file name: 11 characters, `_` at 4 and `.` at 8, and nothing a
/// file system rejects.
pub fn is_name_safe_startup(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 11
        && bytes[4] == b'_'
        && bytes[8] == b'.'
        && bytes
            .iter()
            .all(|&b| b.is_ascii_graphic() && !is_forbidden(char::from(b)))
}

/// How YAOPLM names new images (ADR-0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NameStyle {
    /// `SLUS_200.62.Title.iso`: OPL needs no mount to list it.
    #[default]
    WithStartup,
    /// `Title.iso`.
    TitleOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    #[error("title is empty after removing characters the file system rejects")]
    EmptyTitle,
    #[error("title has {0} characters; OPL allows {TITLE_MAX}")]
    TitleTooLong(usize),
    #[error("game ID {0:?} cannot be stored in an old-style file name")]
    UnsuitableStartup(String),
    #[error(
        "{0:?} looks like an old-style name, so OPL would take its first 11 characters as the game ID"
    )]
    LooksLikeOldFormat(String),
}

/// Builds a file name for a new image. `title` must already be clean (see
/// [`sanitize_title`]); it is validated, not changed.
pub fn build_file_name(
    startup: &str,
    title: &str,
    extension: ImageExtension,
    style: NameStyle,
) -> Result<String, NameError> {
    if title.is_empty() {
        return Err(NameError::EmptyTitle);
    }
    let title_len = title.chars().count();
    if title_len > TITLE_MAX {
        return Err(NameError::TitleTooLong(title_len));
    }
    let name = match style {
        NameStyle::WithStartup => {
            if !is_name_safe_startup(startup) {
                return Err(NameError::UnsuitableStartup(startup.to_owned()));
            }
            format!("{startup}.{title}{extension}")
        }
        NameStyle::TitleOnly => {
            let name = format!("{title}{extension}");
            if classify(&name).is_ok_and(|n| n.format == NameFormat::Old) {
                return Err(NameError::LooksLikeOldFormat(name));
            }
            name
        }
    };
    Ok(name)
}

/// True when the SMB in-game driver can open a file with this name.
pub fn fits_smb(file_name: &str) -> bool {
    file_name.len() <= SMB_FILE_NAME_MAX
}

/// Characters FAT32, exFAT and Windows SMB shares reject in file names.
fn is_forbidden(c: char) -> bool {
    matches!(c, '"' | '*' | '/' | ':' | '<' | '>' | '?' | '\\' | '|') || c.is_control()
}

/// Makes a title safe for FAT32, exFAT and SMB: drops forbidden and control
/// characters, collapses whitespace and trims trailing dots and spaces
/// (Windows strips them silently). Non-ASCII characters are kept; check
/// [`str::is_ascii`] before relying on them.
pub fn sanitize_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut pending_space = false;
    for c in title.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if is_forbidden(c) {
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    let trimmed = out.trim_end_matches(['.', ' ']).len();
    out.truncate(trimmed);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_and_new_formats() {
        let old = classify("SLUS_200.62.Grand Theft Auto III.iso").unwrap();
        assert_eq!(old.format, NameFormat::Old);
        assert_eq!(old.startup.as_deref(), Some("SLUS_200.62"));
        assert_eq!(old.title, "Grand Theft Auto III");
        assert!(old.lowercase_extension());

        let new = classify("Grand Theft Auto III.ZSO").unwrap();
        assert_eq!(new.format, NameFormat::New);
        assert_eq!(new.startup, None);
        assert_eq!(new.title, "Grand Theft Auto III");
        assert_eq!(new.extension, ImageExtension::Zso);
        assert!(!new.lowercase_extension());
    }

    #[test]
    fn old_format_needs_17_chars_and_exact_positions() {
        // 16 chars: ID + "." + ".iso" with an empty title is a new-style name.
        assert_eq!(
            classify("SLUS_200.62..iso").unwrap().format,
            NameFormat::New
        );
        assert_eq!(classify("SLUS_200.62.A.iso").unwrap().title, "A");
        assert_eq!(
            classify("SLUS-200.62.Game.iso").unwrap().format,
            NameFormat::New
        );
        assert_eq!(
            classify("SLUS_20062.Game.iso").unwrap().format,
            NameFormat::New
        );
        // Anything matching the pattern counts, even if it is no real ID.
        let accidental = classify("ABCD_EFG.HI.title.iso").unwrap();
        assert_eq!(accidental.startup.as_deref(), Some("ABCD_EFG.HI"));
    }

    #[test]
    fn rejects_other_extensions_and_short_names() {
        for name in [
            ".iso",
            "a.bin",
            "game.cso",
            "game.iso.bak",
            "iso",
            "game.chd",
        ] {
            assert_eq!(classify(name), Err(NameSkip::NotAnImage), "{name}");
        }
        assert_eq!(classify("a.iso").unwrap().title, "a");
    }

    #[test]
    fn title_limit() {
        let ok = format!("{}.iso", "T".repeat(TITLE_MAX));
        assert!(classify(&ok).is_ok());
        let long = format!("{}.iso", "T".repeat(TITLE_MAX + 1));
        assert_eq!(classify(&long), Err(NameSkip::TitleTooLong));
        // Old style: the limit applies to the title part only.
        let old = format!("SLUS_200.62.{}.iso", "T".repeat(TITLE_MAX));
        assert!(classify(&old).is_ok());
        // Characters, not UTF-8 bytes.
        let cyrillic = format!("{}.iso", "Ж".repeat(TITLE_MAX));
        assert!(classify(&cyrillic).is_ok());
    }

    #[test]
    fn build_names() {
        assert_eq!(
            build_file_name(
                "SLUS_200.62",
                "GTA III",
                ImageExtension::Iso,
                NameStyle::WithStartup
            )
            .unwrap(),
            "SLUS_200.62.GTA III.iso"
        );
        assert_eq!(
            build_file_name(
                "SLUS_200.62",
                "GTA III",
                ImageExtension::Zso,
                NameStyle::TitleOnly
            )
            .unwrap(),
            "GTA III.zso"
        );
        assert!(matches!(
            build_file_name(
                "DATA\\MAIN.E",
                "X",
                ImageExtension::Iso,
                NameStyle::WithStartup
            ),
            Err(NameError::UnsuitableStartup(_))
        ));
        assert!(matches!(
            build_file_name(
                "",
                "ABCD_EFG.HI.x",
                ImageExtension::Iso,
                NameStyle::TitleOnly
            ),
            Err(NameError::LooksLikeOldFormat(_))
        ));
        assert_eq!(
            build_file_name("", "", ImageExtension::Iso, NameStyle::TitleOnly),
            Err(NameError::EmptyTitle)
        );
        let long = "T".repeat(TITLE_MAX + 1);
        assert_eq!(
            build_file_name(
                "SLUS_200.62",
                &long,
                ImageExtension::Iso,
                NameStyle::WithStartup
            ),
            Err(NameError::TitleTooLong(TITLE_MAX + 1))
        );
    }

    #[test]
    fn built_names_classify_back() {
        let name = build_file_name(
            "SCES_500.51",
            "Ratchet",
            ImageExtension::Iso,
            NameStyle::WithStartup,
        )
        .unwrap();
        let parsed = classify(&name).unwrap();
        assert_eq!(parsed.startup.as_deref(), Some("SCES_500.51"));
        assert_eq!(parsed.title, "Ratchet");
    }

    #[test]
    fn smb_limit() {
        let ok = format!("SLUS_200.62.{}.iso", "T".repeat(SMB_FILE_NAME_MAX - 16));
        assert_eq!(ok.len(), SMB_FILE_NAME_MAX);
        assert!(fits_smb(&ok));
        assert!(!fits_smb(&format!("{ok}x")));
    }

    #[test]
    fn sanitize() {
        assert_eq!(
            sanitize_title("  Ratchet:  Gladiator?  "),
            "Ratchet Gladiator"
        );
        assert_eq!(sanitize_title("A/B\\C|D\"E*F<G>H"), "ABCDEFGH");
        assert_eq!(
            sanitize_title("Tom Clancy's\tSplinter\nCell..."),
            "Tom Clancy's Splinter Cell"
        );
        assert_eq!(sanitize_title("Ōkami"), "Ōkami");
        assert_eq!(sanitize_title("???"), "");
    }
}
