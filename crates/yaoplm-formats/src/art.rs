//! Art file names and recommended sizes (`ART/<key>_<SUFFIX>.png`).
//!
//! Format reference: `docs/research/02-opl-formats.md` §7 and research/05
//! §3.4. Mirrors `bdmGetImage()` (`src/bdmsupport.c`, path
//! `"%s%s/%s_%s"`) and `texDiscoverLoad()`/`texSizeValidate()`
//! (`src/textures.c`) of ps2homebrew/Open-PS2-Loader@b3a21997. RiptOPL keys
//! follow the fork author's spec ("Standards and Specifications for RiptOPL
//! + PSBBN-DP", psx-place, 2026-09-25).
//!
//! - Games are keyed by game ID, apps by the boot ELF file name including
//!   `.ELF`.
//! - RiptOPL adds VCD games (file name without `.VCD`) and Ember games
//!   (folder name in `EMBER/games/`).
//! - OPL rejects images wider or taller than 1024 pixels.
//! - Pixel sizes are recommendations (OPL Manager docs, RiptOPL spec); OPL
//!   scales whatever it gets.

use std::fmt;

use crate::profile::LoaderProfile;

pub const ART_FOLDER: &str = "ART";
/// `texSizeValidate()`.
pub const MAX_DIMENSION: u32 = 1024;

/// Art types known to OPL themes. The built-in theme uses `COV`, `ICO`,
/// `BG`, `SCR` and `SCR2`; custom themes can ask for any suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKind {
    /// Front cover.
    Cov,
    /// Back cover.
    Cov2,
    /// Spine.
    Lab,
    Ico,
    Lgo,
    Scr,
    Scr2,
    Bg,
}

impl ArtKind {
    pub const ALL: [Self; 8] = [
        Self::Cov,
        Self::Cov2,
        Self::Lab,
        Self::Ico,
        Self::Lgo,
        Self::Scr,
        Self::Scr2,
        Self::Bg,
    ];

    pub fn suffix(self) -> &'static str {
        match self {
            Self::Cov => "COV",
            Self::Cov2 => "COV2",
            Self::Lab => "LAB",
            Self::Ico => "ICO",
            Self::Lgo => "LGO",
            Self::Scr => "SCR",
            Self::Scr2 => "SCR2",
            Self::Bg => "BG",
        }
    }

    pub fn from_suffix(suffix: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.suffix() == suffix)
    }
}

impl fmt::Display for ArtKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// What the art belongs to; decides the key in the file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtSubject<'a> {
    /// A PS2 game: its ID, e.g. `SLUS_200.62`.
    Game { startup: &'a str },
    /// An app: the `boot=` ELF name from `title.cfg`, e.g. `OPNPS2LD.ELF`.
    App { boot: &'a str },
    /// RiptOPL PS1 game through POPStarter: `Crash Bandicoot.VCD`.
    Vcd { file_name: &'a str },
    /// RiptOPL PS1 game through Ember: the folder in `EMBER/games/`.
    Ember { folder: &'a str },
}

impl<'a> ArtSubject<'a> {
    pub fn key(self) -> &'a str {
        match self {
            Self::Game { startup } => startup,
            Self::App { boot } => boot,
            Self::Vcd { file_name } => {
                let cut = file_name.len().saturating_sub(4);
                match file_name.get(cut..) {
                    Some(ext) if ext.eq_ignore_ascii_case(".vcd") => &file_name[..cut],
                    _ => file_name,
                }
            }
            Self::Ember { folder } => folder,
        }
    }

    /// PS1 games and apps use square covers.
    pub fn is_square(self) -> bool {
        !matches!(self, Self::Game { .. })
    }
}

/// `<key>_<SUFFIX>.png`, the only name YAOPLM writes.
pub fn art_file_name(key: &str, kind: ArtKind) -> String {
    format!("{key}_{}.png", kind.suffix())
}

/// An art file name split into key, suffix and extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtFileName {
    pub key: String,
    pub suffix: String,
    pub extension: String,
}

impl ArtFileName {
    pub fn kind(&self) -> Option<ArtKind> {
        ArtKind::from_suffix(&self.suffix)
    }

    /// Whether `profile` loads this file at all.
    pub fn loads_in(&self, profile: LoaderProfile) -> bool {
        profile
            .art_extensions()
            .iter()
            .any(|ext| self.extension.eq_ignore_ascii_case(ext))
    }
}

/// Splits at the last `_` before the extension: IDs contain `_` themselves.
pub fn parse_art_file_name(name: &str) -> Option<ArtFileName> {
    let (stem, extension) = name.rsplit_once('.')?;
    let (key, suffix) = stem.rsplit_once('_')?;
    if key.is_empty() || suffix.is_empty() || extension.is_empty() {
        return None;
    }
    Some(ArtFileName {
        key: key.to_owned(),
        suffix: suffix.to_owned(),
        extension: extension.to_owned(),
    })
}

/// A picture in a picture set (ART-5): whose it is and of which type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackPicture {
    /// The game ID as the file names it, either spelling.
    pub key: String,
    pub kind: ArtKind,
    /// Lower case: `png`, `jpg`...
    pub extension: String,
}

/// Image formats a picture set may hold; they are transcoded to PNG.
pub const PACK_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "gif", "webp"];

/// Reads a file name of a picture set, wherever it sits in the set:
/// `SLUS_200.62_COV.png` as OPL names art, and OPL Manager's backups with
/// numbered screenshots and backgrounds, `SLUS_200.62_SCR_00.jpg` (the
/// first two are SCR and SCR2, the first background is BG; further ones
/// have no place in OPL). Case does not matter, as in RiptOPL's tar
/// lookups. Other names, and other types, give `None`.
pub fn pack_picture(file_name: &str) -> Option<PackPicture> {
    let (stem, extension) = file_name.rsplit_once('.')?;
    let extension = extension.to_ascii_lowercase();
    if !PACK_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    let (rest, last) = stem.rsplit_once('_')?;
    let numbered = last.len() == 2 && last.bytes().all(|b| b.is_ascii_digit());
    let (key, kind) = if numbered {
        let (key, suffix) = rest.rsplit_once('_')?;
        let kind = match (suffix.to_ascii_uppercase().as_str(), last) {
            ("SCR", "00") => ArtKind::Scr,
            ("SCR", "01") => ArtKind::Scr2,
            ("BG", "00") => ArtKind::Bg,
            _ => return None,
        };
        (key, kind)
    } else {
        (rest, ArtKind::from_suffix(&last.to_ascii_uppercase())?)
    };
    if key.is_empty() {
        return None;
    }
    Some(PackPicture {
        key: key.to_owned(),
        kind,
        extension,
    })
}

/// The IHDR fields of a PNG file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngHeader {
    pub width: u32,
    pub height: u32,
    /// Bits per channel or palette index. RiptOPL rejects more than 8;
    /// upstream OPL strips 16 to 8.
    pub bit_depth: u8,
    pub color_type: u8,
}

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Reads a PNG header from the first 26 bytes of a file.
pub fn png_header(bytes: &[u8]) -> Option<PngHeader> {
    if bytes.len() < 26 || &bytes[..8] != PNG_SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be32 = |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    Some(PngHeader {
        width: be32(16),
        height: be32(20),
        bit_depth: bytes[24],
        color_type: bytes[25],
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtSize {
    pub width: u32,
    pub height: u32,
}

const fn size(width: u32, height: u32) -> ArtSize {
    ArtSize { width, height }
}

/// Recommended pixel size for new art.
pub fn recommended_size(profile: LoaderProfile, subject: ArtSubject<'_>, kind: ArtKind) -> ArtSize {
    let square = subject.is_square();
    let ript = profile == LoaderProfile::RiptOpl;
    match kind {
        ArtKind::Cov => match (ript, square) {
            (false, false) => size(140, 200),
            (false, true) => size(200, 200),
            (true, false) => size(280, 400),
            (true, true) => size(400, 400),
        },
        ArtKind::Cov2 if square => size(222, 200),
        ArtKind::Cov2 => size(242, 344),
        ArtKind::Lab if square => size(12, 200),
        ArtKind::Lab => size(18, 240),
        ArtKind::Ico if ript => size(128, 128),
        ArtKind::Ico => size(64, 64),
        ArtKind::Lgo => size(300, 125),
        ArtKind::Scr | ArtKind::Scr2 => size(250, 188),
        ArtKind::Bg => size(640, 480),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let game = ArtSubject::Game {
            startup: "SLUS_200.62",
        };
        assert_eq!(
            art_file_name(game.key(), ArtKind::Cov),
            "SLUS_200.62_COV.png"
        );
        let app = ArtSubject::App {
            boot: "XX.Crash Bandicoot.ELF",
        };
        assert_eq!(
            art_file_name(app.key(), ArtKind::Ico),
            "XX.Crash Bandicoot.ELF_ICO.png"
        );
        let vcd = ArtSubject::Vcd {
            file_name: "Spyro 2 (Ripto's Rage).VCD",
        };
        assert_eq!(
            art_file_name(vcd.key(), ArtKind::Cov),
            "Spyro 2 (Ripto's Rage)_COV.png"
        );
        assert_eq!(ArtSubject::Vcd { file_name: "a.vcd" }.key(), "a");
        assert_eq!(
            ArtSubject::Ember {
                folder: "Crash Bandicoot"
            }
            .key(),
            "Crash Bandicoot"
        );
    }

    #[test]
    fn parse_names() {
        let parsed = parse_art_file_name("SLUS_200.62_COV2.png").unwrap();
        assert_eq!(parsed.key, "SLUS_200.62");
        assert_eq!(parsed.kind(), Some(ArtKind::Cov2));
        assert!(parsed.loads_in(LoaderProfile::Opl120Beta));

        let jpg = parse_art_file_name("SLUS_200.62_COV.jpg").unwrap();
        assert!(!jpg.loads_in(LoaderProfile::Opl120Beta));
        assert!(jpg.loads_in(LoaderProfile::Opl110));

        let custom = parse_art_file_name("OPNPS2LD.ELF_LOGO2.png").unwrap();
        assert_eq!(custom.key, "OPNPS2LD.ELF");
        assert_eq!(custom.kind(), None);

        for bad in [
            "COV.png",
            "_COV.png",
            "SLUS_.png",
            "noext",
            "SLUS_200.62_COV.",
        ] {
            assert_eq!(parse_art_file_name(bad), None, "{bad}");
        }
    }

    #[test]
    fn picture_set_names() {
        let pic = |name| pack_picture(name).map(|p| (p.key, p.kind, p.extension));
        assert_eq!(
            pic("SLUS_200.62_COV.png"),
            Some(("SLUS_200.62".into(), ArtKind::Cov, "png".into()))
        );
        assert_eq!(
            pic("slus_200.62_cov2.JPG"),
            Some(("slus_200.62".into(), ArtKind::Cov2, "jpg".into()))
        );
        // OPL Manager's numbered screenshots and backgrounds.
        assert_eq!(pic("SLUS_200.62_SCR_00.jpg").unwrap().1, ArtKind::Scr);
        assert_eq!(pic("SLUS_200.62_SCR_01.jpg").unwrap().1, ArtKind::Scr2);
        assert_eq!(pic("SLUS_200.62_BG_00.jpg").unwrap().1, ArtKind::Bg);
        assert_eq!(pic("SLUS_200.62_SCR_02.jpg"), None);
        assert_eq!(pic("SLUS_200.62_BG_01.png"), None);
        // An ID ending in two digits is not mistaken for a number.
        assert_eq!(
            pic("SLPM_123.45_ICO.png"),
            Some(("SLPM_123.45".into(), ArtKind::Ico, "png".into()))
        );
        for bad in [
            "PS2_LIST.CSV",
            "SLUS_200.62_COV.txt",
            "SLUS_200.62_LOGO2.png",
            "_COV.png",
            "COV.png",
            "SLUS_200.62_COV",
        ] {
            assert_eq!(pack_picture(bad), None, "{bad}");
        }
    }

    #[test]
    fn png_headers() {
        let mut png = PNG_SIGNATURE.to_vec();
        png.extend_from_slice(b"\0\0\0\x0dIHDR");
        png.extend_from_slice(&140u32.to_be_bytes());
        png.extend_from_slice(&200u32.to_be_bytes());
        png.extend_from_slice(&[8, 6]);
        let header = png_header(&png).unwrap();
        assert_eq!(
            (header.width, header.height, header.bit_depth),
            (140, 200, 8)
        );
        assert_eq!(png_header(&png[..20]), None);
        assert_eq!(png_header(&[0xFF; 30]), None);
    }

    #[test]
    fn sizes() {
        let game = ArtSubject::Game { startup: "X" };
        let app = ArtSubject::App { boot: "A.ELF" };
        let opl = LoaderProfile::Opl120Beta;
        let ript = LoaderProfile::RiptOpl;
        assert_eq!(recommended_size(opl, game, ArtKind::Cov), size(140, 200));
        assert_eq!(recommended_size(opl, app, ArtKind::Cov), size(200, 200));
        assert_eq!(recommended_size(ript, game, ArtKind::Cov), size(280, 400));
        assert_eq!(recommended_size(ript, app, ArtKind::Cov), size(400, 400));
        assert_eq!(recommended_size(ript, game, ArtKind::Ico), size(128, 128));
        assert_eq!(recommended_size(opl, game, ArtKind::Ico), size(64, 64));
        // Only COV and ICO double in RiptOPL.
        assert_eq!(
            recommended_size(ript, game, ArtKind::Cov2),
            recommended_size(opl, game, ArtKind::Cov2)
        );
        assert_eq!(recommended_size(ript, app, ArtKind::Lab), size(12, 200));
        for kind in ArtKind::ALL {
            for profile in LoaderProfile::ALL {
                let s = recommended_size(profile, game, kind);
                assert!(s.width <= MAX_DIMENSION && s.height <= MAX_DIMENSION);
            }
        }
    }
}
