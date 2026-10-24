//! Where pictures come from (ART-1, ART-2, ADR-0007, ADR-0035, research/03
//! §2, research/14, SPEC §5.4.1).
//!
//! Four sources:
//! 1. `xlenore/ps2-covers`: front covers (COV) by serial,
//!    `covers/default/<SERIAL>.jpg`, the URL template PCSX2's own cover
//!    downloader uses, and 3-D renders in `covers/3d/<SERIAL>.png`,
//!    offered as another COV to choose from. No licence file, so it is
//!    fetched only when the user asks and never bundled.
//! 2. OPL Manager's art backup `OPLM_ART_2024_09` on archive.org: every
//!    type, by OPL ID, read member by member ([`crate::oplm`]). The only
//!    keyless source of icons, back covers, spines and backgrounds.
//! 3. `libretro-thumbnails/Sony_-_PlayStation_2`: `<folder>/<Redump
//!    name>.png`, served by `thumbnails.libretro.com` with the raw GitHub
//!    URL as a fallback: `Named_Boxarts` (COV), `Named_Snaps` (SCR, and BG
//!    when nothing else has one), `Named_Titles` (SCR2), `Named_Logos`
//!    (LGO). The names come from the bundled DAT (META-1).
//! 4. ScreenScraper: every type by serial, with the market of each
//!    picture, once a developer key is set up ([`crate::screenscraper`]).
//!
//! The user does not pick a source: each type has its own order
//! ([`order`]), and the first source with a fitting picture wins. Packaging
//! (COV, COV2, LAB, ICO) only comes from a match on the game's ID or
//! serial, or on a name tagged with its market ([`crate::region`]).
//!
//! None goes through the GitHub API (60 requests per hour unauthenticated,
//! which is where OrbitPS2 Manager's bulk downloads stall, research/11).
//! Each source but ScreenScraper can instead read a local mirror: a clone
//! of the repository, or any folder with the same layout. The mirror is
//! also what the tests and the e2e fixture use, so CI never touches the
//! network.

use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use yaoplm_formats::art::{ArtKind, ArtSize, ArtSubject, recommended_size};
use yaoplm_formats::profile::LoaderProfile;
use yaoplm_formats::serial;

use crate::region;
use crate::screenscraper::Account;

/// A source id, stable on the wire and in the cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceId {
    Xlenore,
    Oplm,
    Libretro,
    ScreenScraper,
}

impl SourceId {
    pub const ALL: [Self; 4] = [
        Self::Xlenore,
        Self::Oplm,
        Self::Libretro,
        Self::ScreenScraper,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Xlenore => "xlenore",
            Self::Oplm => "oplm",
            Self::Libretro => "libretro",
            Self::ScreenScraper => "screenscraper",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == id)
    }

    /// The picture types the source has.
    pub fn kinds(self) -> &'static [ArtKind] {
        match self {
            Self::Xlenore => &[ArtKind::Cov],
            Self::Libretro => &[
                ArtKind::Cov,
                ArtKind::Scr,
                ArtKind::Scr2,
                ArtKind::Lgo,
                ArtKind::Bg,
            ],
            Self::Oplm | Self::ScreenScraper => &ArtKind::ALL,
        }
    }

    /// Finds pictures by the Redump names of the bundled database, so a
    /// game without one is not looked up there.
    pub fn needs_redump_name(self) -> bool {
        self == Self::Libretro
    }

    /// The size of the source's pictures of `kind`, when they all have
    /// one: OPL Manager's backup holds them at OPL's sizes.
    pub fn native_size(self, kind: ArtKind) -> Option<ArtSize> {
        (self == Self::Oplm).then(|| {
            recommended_size(
                LoaderProfile::Opl120Beta,
                ArtSubject::Game { startup: "" },
                kind,
            )
        })
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
            Self::Oplm => SourceInfo {
                id: self,
                name: "OPL Manager art backup (archive.org)",
                url: "https://archive.org/details/OPLM_ART_2024_09",
                license_tag: "unlicensed",
                attribution: "OPL Manager ART database users, backup OPLM_ART_2024_09 by danielb",
            },
            Self::ScreenScraper => SourceInfo {
                id: self,
                name: "ScreenScraper",
                url: "https://www.screenscraper.fr",
                license_tag: "CC BY-NC-SA 4.0",
                attribution: "ScreenScraper.fr contributors",
            },
        }
    }
}

/// The sources for `kind`, best first (ADR-0035, research/14 §5):
///
/// | Type | Order |
/// |---|---|
/// | COV | xlenore, OPL Manager, libretro, ScreenScraper |
/// | ICO, COV2, LAB | OPL Manager, ScreenScraper |
/// | BG | OPL Manager, ScreenScraper, libretro (a screenshot) |
/// | SCR, SCR2, LGO | libretro, OPL Manager, ScreenScraper |
///
/// A source whose pictures are smaller than the slot goes after the
/// others: OPL Manager's 140x200 covers and 64x64 icons come last for
/// RiptOPL's 280x400 and 128x128.
pub fn order(kind: ArtKind, size: ArtSize) -> Vec<SourceId> {
    use SourceId::{Libretro, Oplm, ScreenScraper, Xlenore};
    let base: &[SourceId] = match kind {
        ArtKind::Cov => &[Xlenore, Oplm, Libretro, ScreenScraper],
        ArtKind::Ico | ArtKind::Cov2 | ArtKind::Lab => &[Oplm, ScreenScraper],
        ArtKind::Bg => &[Oplm, ScreenScraper, Libretro],
        ArtKind::Scr | ArtKind::Scr2 | ArtKind::Lgo => &[Libretro, Oplm, ScreenScraper],
    };
    let small = |source: &SourceId| {
        source
            .native_size(kind)
            .is_some_and(|n| n.width < size.width || n.height < size.height)
    };
    let (mut out, smaller): (Vec<SourceId>, Vec<SourceId>) =
        base.iter().copied().partition(|s| !small(s));
    out.extend(smaller);
    out
}

/// The configured sources that have `kind`, in the order of [`order`].
pub fn ordered(configs: &[SourceConfig], kind: ArtKind, size: ArtSize) -> Vec<&SourceConfig> {
    order(kind, size)
        .into_iter()
        .filter_map(|id| configs.iter().find(|c| c.id == id))
        .collect()
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
    /// The user's ScreenScraper account, if they have one.
    pub account: Option<Account>,
}

impl SourceConfig {
    pub fn remote(id: SourceId) -> Self {
        Self {
            id,
            location: Location::Remote,
            account: None,
        }
    }

    pub fn mirror(id: SourceId, dir: impl Into<PathBuf>) -> Self {
        Self {
            id,
            location: Location::Mirror(dir.into()),
            account: None,
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

/// Where `source` keeps pictures of `kind` for a game, best first, in a
/// repository's layout. With `variants`, also the alternatives a user may
/// pick from instead (the 3-D covers). For packaging, libretro is asked
/// only for names tagged with the game's market ([`region::name_fits`]).
/// OPL Manager's backup is listed by its mirror layout; read remotely, its
/// index decides ([`crate::oplm`]). ScreenScraper has no fixed layout.
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
        SourceId::Xlenore | SourceId::ScreenScraper => Vec::new(),
        SourceId::Libretro => {
            let names: Vec<String> = redump_names
                .iter()
                .filter(|name| region::name_fits(kind, serial, name))
                .cloned()
                .collect();
            libretro_folder(kind)
                .map(|folder| libretro_candidates_in(folder, &names))
                .unwrap_or_default()
        }
        SourceId::Oplm => crate::oplm::mirror_paths(&serial::serial_to_opl(serial), kind)
            .into_iter()
            .map(|(path, key)| Candidate {
                key,
                path,
                urls: Vec::new(),
            })
            .collect(),
    }
}

/// The libretro-thumbnails folder of `kind`.
fn libretro_folder(kind: ArtKind) -> Option<&'static str> {
    match kind {
        ArtKind::Cov => Some("Named_Boxarts"),
        // A gameplay frame stands in for a background no source has.
        ArtKind::Scr | ArtKind::Bg => Some("Named_Snaps"),
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
        assert_eq!(SourceId::parse("oplm"), Some(SourceId::Oplm));
        assert_eq!(SourceId::parse("launchbox"), None);
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
        // Packaging beyond the front cover: OPL Manager's backup only (and
        // ScreenScraper, which has no fixed layout).
        for kind in [ArtKind::Ico, ArtKind::Cov2, ArtKind::Lab] {
            for source in [SourceId::Xlenore, SourceId::Libretro] {
                assert!(
                    candidates(source, kind, "SCUS-97113", &names, true).is_empty(),
                    "{kind}"
                );
                assert!(!source.kinds().contains(&kind));
            }
            assert!(SourceId::Oplm.kinds().contains(&kind));
            assert!(SourceId::ScreenScraper.kinds().contains(&kind));
        }
        // A background no one has: a gameplay frame.
        let bg = candidates(SourceId::Libretro, ArtKind::Bg, "SCUS-97113", &names, false);
        assert_eq!(bg[0].key, "Named_Snaps/Ico (USA)");
        let lab = candidates(SourceId::Oplm, ArtKind::Lab, "SCUS-97113", &names, false);
        assert_eq!(lab[0].path, "PS2/SCUS_971.13/SCUS_971.13_LAB.png");
        assert_eq!(lab[1].path, "PS2/SCUS_971.13/SCUS_971.13_LAB.jpg");
        assert_eq!(lab[0].key, "SCUS_971.13_LAB");
        assert!(SourceId::Libretro.needs_redump_name() && !SourceId::Oplm.needs_redump_name());
    }

    #[test]
    fn packaging_names_keep_to_the_games_market() {
        let names = [
            "Dead or Alive 2 (Japan)".to_owned(),
            "Dead or Alive 2 (Europe) (En,Fr,De,Es,It)".to_owned(),
        ];
        let paths = |kind, serial| -> Vec<String> {
            candidates(SourceId::Libretro, kind, serial, &names, false)
                .into_iter()
                .map(|c| c.path)
                .collect()
        };
        assert_eq!(
            paths(ArtKind::Cov, "SLPS-25002"),
            ["Named_Boxarts/Dead or Alive 2 (Japan).png"]
        );
        assert_eq!(
            paths(ArtKind::Cov, "SCES-50003"),
            ["Named_Boxarts/Dead or Alive 2 (Europe) (En,Fr,De,Es,It).png"]
        );
        assert!(paths(ArtKind::Cov, "SLUS-20071").is_empty());
        // Screenshots are the same game everywhere.
        assert_eq!(paths(ArtKind::Scr, "SLUS-20071").len(), 2);
    }

    #[test]
    fn order_per_kind_and_size() {
        let size = |w, h| ArtSize {
            width: w,
            height: h,
        };
        use SourceId::{Libretro, Oplm, ScreenScraper, Xlenore};
        assert_eq!(
            order(ArtKind::Cov, size(140, 200)),
            [Xlenore, Oplm, Libretro, ScreenScraper]
        );
        // RiptOPL's covers are twice OPL Manager's.
        assert_eq!(
            order(ArtKind::Cov, size(280, 400)),
            [Xlenore, Libretro, ScreenScraper, Oplm]
        );
        assert_eq!(order(ArtKind::Ico, size(64, 64)), [Oplm, ScreenScraper]);
        assert_eq!(order(ArtKind::Ico, size(128, 128)), [ScreenScraper, Oplm]);
        assert_eq!(order(ArtKind::Lab, size(18, 240)), [Oplm, ScreenScraper]);
        assert_eq!(
            order(ArtKind::Bg, size(640, 480)),
            [Oplm, ScreenScraper, Libretro]
        );
        assert_eq!(
            order(ArtKind::Scr2, size(250, 188)),
            [Libretro, Oplm, ScreenScraper]
        );
        // Every source of a type is in its order, and nothing else.
        for kind in ArtKind::ALL {
            let mut listed = order(kind, size(1, 1));
            listed.sort_by_key(|s| s.as_str());
            let mut having: Vec<SourceId> = SourceId::ALL
                .into_iter()
                .filter(|s| s.kinds().contains(&kind))
                .collect();
            having.sort_by_key(|s| s.as_str());
            assert_eq!(listed, having, "{kind}");
        }
        let configs = [
            SourceConfig::remote(Libretro),
            SourceConfig::mirror(Oplm, "m"),
            SourceConfig::remote(Xlenore),
        ];
        let ids: Vec<SourceId> = ordered(&configs, ArtKind::Scr, size(250, 188))
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, [Libretro, Oplm]);
    }
}
