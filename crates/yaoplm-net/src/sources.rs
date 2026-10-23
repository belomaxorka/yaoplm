//! Where pictures come from (ART-1, ART-2, ADR-0007, research/03 §2.2-2.3,
//! SPEC §5.4.1).
//!
//! Two sources, tried in order:
//! 1. `xlenore/ps2-covers`: front covers (COV) by serial,
//!    `covers/default/<SERIAL>.jpg`, the URL template PCSX2's own cover
//!    downloader uses, and 3-D renders in `covers/3d/<SERIAL>.png`,
//!    offered as another COV to choose from. No licence file, so it is
//!    fetched only when the user asks and never bundled.
//! 2. `libretro-thumbnails/Sony_-_PlayStation_2`: `<folder>/<Redump
//!    name>.png`, served by `thumbnails.libretro.com` with the raw GitHub
//!    URL as a fallback: `Named_Boxarts` (COV), `Named_Snaps` (SCR),
//!    `Named_Titles` (SCR2), `Named_Logos` (LGO). The names come from the
//!    bundled DAT (META-1).
//!
//! ICO, COV2, LAB and BG have no open source: art packs and the user's own
//! files fill them (ART-3, ART-5).
//!
//! Neither goes through the GitHub API (60 requests per hour unauthenticated,
//! which is where OrbitPS2 Manager's bulk downloads stall, research/11).
//! Each source can instead read a local mirror: a clone of the repository,
//! or any folder with the same layout. The mirror is also what the tests
//! and the e2e fixture use, so CI never touches the network.

use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use yaoplm_formats::art::ArtKind;

/// A source id, stable on the wire and in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceId {
    Xlenore,
    Libretro,
}

impl SourceId {
    pub const ALL: [Self; 2] = [Self::Xlenore, Self::Libretro];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xlenore => "xlenore",
            Self::Libretro => "libretro",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == id)
    }

    /// The picture types the source has.
    pub fn kinds(self) -> &'static [ArtKind] {
        match self {
            Self::Xlenore => &[ArtKind::Cov],
            Self::Libretro => &[ArtKind::Cov, ArtKind::Scr, ArtKind::Scr2, ArtKind::Lgo],
        }
    }

    pub fn info(self) -> SourceInfo {
        match self {
            Self::Xlenore => SourceInfo {
                id: self,
                name: "xlenore/ps2-covers",
                url: "https://github.com/xlenore/ps2-covers",
                license_tag: "unlicensed",
                attribution: "xlenore/ps2-covers (pcsx2.net, psxdatacenter.com, gvcover.top, imkira3)",
            },
            Self::Libretro => SourceInfo {
                id: self,
                name: "libretro-thumbnails",
                url: "https://github.com/libretro-thumbnails/Sony_-_PlayStation_2",
                license_tag: "unlicensed",
                attribution: "libretro-thumbnails contributors",
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceInfo {
    pub id: SourceId,
    pub name: &'static str,
    pub url: &'static str,
    /// `unlicensed`: use on request only, never redistribute.
    pub license_tag: &'static str,
    pub attribution: &'static str,
}

/// Where a source reads from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Remote,
    /// A folder with the repository's layout.
    Mirror(PathBuf),
}

/// How one source is configured for a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceConfig {
    pub id: SourceId,
    pub location: Location,
}

impl SourceConfig {
    pub fn remote(id: SourceId) -> Self {
        Self {
            id,
            location: Location::Remote,
        }
    }

    pub fn mirror(id: SourceId, dir: impl Into<PathBuf>) -> Self {
        Self {
            id,
            location: Location::Mirror(dir.into()),
        }
    }
}

/// One place to look: a URL, or a file inside a mirror.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The cache key within the source.
    pub key: String,
    /// Repository-relative path, with `/` separators.
    pub path: String,
    /// URLs to try in order (remote only).
    pub urls: Vec<String>,
}

impl Candidate {
    pub fn mirror_path(&self, mirror: &Path) -> PathBuf {
        let mut path = mirror.to_path_buf();
        path.extend(self.path.split('/'));
        path
    }
}

/// `covers/default/<SERIAL>.jpg`.
pub fn xlenore_candidates(serial: &str) -> Vec<Candidate> {
    vec![xlenore(
        format!("covers/default/{serial}.jpg"),
        serial.to_owned(),
    )]
}

fn xlenore(path: String, key: String) -> Candidate {
    Candidate {
        key,
        urls: vec![format!(
            "https://raw.githubusercontent.com/xlenore/ps2-covers/main/{path}"
        )],
        path,
    }
}

/// Where `source` keeps pictures of `kind` for a game, best first. With
/// `variants`, also the alternatives a user may pick from instead (the
/// 3-D covers).
pub fn candidates(
    source: SourceId,
    kind: ArtKind,
    serial: &str,
    redump_names: &[String],
    variants: bool,
) -> Vec<Candidate> {
    match source {
        SourceId::Xlenore if kind == ArtKind::Cov => {
            let mut out = xlenore_candidates(serial);
            if variants {
                // Keyed apart from the 2-D cover, which keeps the plain
                // serial key existing caches have.
                out.push(xlenore(
                    format!("covers/3d/{serial}.png"),
                    format!("3d/{serial}"),
                ));
            }
            out
        }
        SourceId::Xlenore => Vec::new(),
        SourceId::Libretro => libretro_folder(kind)
            .map(|folder| libretro_candidates_in(folder, redump_names))
            .unwrap_or_default(),
    }
}

/// The libretro-thumbnails folder of `kind`.
fn libretro_folder(kind: ArtKind) -> Option<&'static str> {
    match kind {
        ArtKind::Cov => Some("Named_Boxarts"),
        ArtKind::Scr => Some("Named_Snaps"),
        ArtKind::Scr2 => Some("Named_Titles"),
        ArtKind::Lgo => Some("Named_Logos"),
        _ => None,
    }
}

/// Characters libretro replaces with `_` in thumbnail file names
/// (libretro-thumbnails README): `&*/:`<>?\|"`.
pub fn libretro_file_name(redump_name: &str) -> String {
    redump_name
        .chars()
        .map(|c| match c {
            '&' | '*' | '/' | ':' | '`' | '<' | '>' | '?' | '\\' | '|' | '"' => '_',
            c => c,
        })
        .collect()
}

/// Everything but unreserved characters is percent-encoded; spaces too.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// `Named_Boxarts/<name>.png` for each Redump name, in the order given.
pub fn libretro_candidates(redump_names: &[String]) -> Vec<Candidate> {
    libretro_candidates_in("Named_Boxarts", redump_names)
}

/// `<folder>/<name>.png` for each Redump name. Every folder has the same
/// file names, so the cache key carries the folder; the box art keeps the
/// bare name existing caches have.
fn libretro_candidates_in(folder: &str, redump_names: &[String]) -> Vec<Candidate> {
    redump_names
        .iter()
        .map(|name| {
            let file = format!("{}.png", libretro_file_name(name));
            let encoded = utf8_percent_encode(&file, PATH_SEGMENT).to_string();
            Candidate {
                key: if folder == "Named_Boxarts" {
                    name.clone()
                } else {
                    format!("{folder}/{name}")
                },
                path: format!("{folder}/{file}"),
                urls: vec![
                    format!("https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/{folder}/{encoded}"),
                    format!("https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/master/{folder}/{encoded}"),
                ],
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlenore_urls() {
        let c = xlenore_candidates("SLUS-20062");
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].urls[0],
            "https://raw.githubusercontent.com/xlenore/ps2-covers/main/covers/default/SLUS-20062.jpg"
        );
        assert_eq!(
            c[0].mirror_path(Path::new("m")),
            Path::new("m")
                .join("covers")
                .join("default")
                .join("SLUS-20062.jpg")
        );
    }

    #[test]
    fn libretro_names_and_urls() {
        assert_eq!(
            libretro_file_name("Ico (USA) & Co: Return? \"Q\""),
            "Ico (USA) _ Co_ Return_ _Q_"
        );
        let c = libretro_candidates(&["Grand Theft Auto III (USA, Canada)".into()]);
        assert_eq!(
            c[0].path,
            "Named_Boxarts/Grand Theft Auto III (USA, Canada).png"
        );
        assert_eq!(
            c[0].urls[0],
            "https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/Named_Boxarts/Grand%20Theft%20Auto%20III%20%28USA%2C%20Canada%29.png"
        );
        assert!(c[0].urls[1].starts_with("https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/master/Named_Boxarts/"));
        assert_eq!(SourceId::parse("libretro"), Some(SourceId::Libretro));
        assert_eq!(SourceId::parse("oplm"), None);
    }

    #[test]
    fn folders_per_kind() {
        let names = ["Ico (USA)".to_owned()];
        let snaps = candidates(
            SourceId::Libretro,
            ArtKind::Scr,
            "SCUS-97113",
            &names,
            false,
        );
        assert_eq!(snaps[0].path, "Named_Snaps/Ico (USA).png");
        assert_eq!(snaps[0].key, "Named_Snaps/Ico (USA)");
        assert!(snaps[0].urls[0].contains("/Named_Snaps/Ico%20%28USA%29.png"));
        let titles = candidates(SourceId::Libretro, ArtKind::Scr2, "", &names, false);
        assert_eq!(titles[0].path, "Named_Titles/Ico (USA).png");
        let logos = candidates(SourceId::Libretro, ArtKind::Lgo, "", &names, false);
        assert_eq!(logos[0].key, "Named_Logos/Ico (USA)");
        // The box art keeps the key caches already have.
        let boxes = candidates(SourceId::Libretro, ArtKind::Cov, "", &names, false);
        assert_eq!(boxes[0].key, "Ico (USA)");
        // xlenore has front covers only; 3-D ones are a variant.
        assert!(candidates(SourceId::Xlenore, ArtKind::Scr, "SCUS-97113", &names, true).is_empty());
        let covers = candidates(SourceId::Xlenore, ArtKind::Cov, "SCUS-97113", &names, true);
        assert_eq!(covers.len(), 2);
        assert_eq!(covers[1].path, "covers/3d/SCUS-97113.png");
        assert_eq!(covers[1].key, "3d/SCUS-97113");
        assert_eq!(
            candidates(SourceId::Xlenore, ArtKind::Cov, "SCUS-97113", &names, false).len(),
            1
        );
        for kind in [ArtKind::Ico, ArtKind::Cov2, ArtKind::Lab, ArtKind::Bg] {
            for source in SourceId::ALL {
                assert!(
                    candidates(source, kind, "X", &names, true).is_empty(),
                    "{kind}"
                );
                assert!(!source.kinds().contains(&kind));
            }
        }
    }
}
