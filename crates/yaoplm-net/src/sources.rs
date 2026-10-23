//! Where covers come from (ART-1, ADR-0007, research/03 §2.2-2.3).
//!
//! Two sources, tried in order:
//! 1. `xlenore/ps2-covers`: `covers/default/<SERIAL>.jpg` by serial, the
//!    URL template PCSX2's own cover downloader uses. No licence file, so
//!    it is fetched only when the user asks and never bundled.
//! 2. `libretro-thumbnails/Sony_-_PlayStation_2`: `Named_Boxarts/<Redump
//!    name>.png`, served by `thumbnails.libretro.com` with the raw GitHub
//!    URL as a fallback. The names come from the bundled DAT (META-1).
//!
//! Neither goes through the GitHub API (60 requests per hour unauthenticated,
//! which is where OrbitPS2 Manager's bulk downloads stall, research/11).
//! Each source can instead read a local mirror: a clone of the repository,
//! or any folder with the same layout. The mirror is also what the tests
//! and the e2e fixture use, so CI never touches the network.

use std::path::{Path, PathBuf};

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

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
    let path = format!("covers/default/{serial}.jpg");
    vec![Candidate {
        key: serial.to_owned(),
        urls: vec![format!(
            "https://raw.githubusercontent.com/xlenore/ps2-covers/main/{path}"
        )],
        path,
    }]
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
    redump_names
        .iter()
        .map(|name| {
            let file = format!("{}.png", libretro_file_name(name));
            let encoded = utf8_percent_encode(&file, PATH_SEGMENT).to_string();
            Candidate {
                key: name.clone(),
                path: format!("Named_Boxarts/{file}"),
                urls: vec![
                    format!("https://thumbnails.libretro.com/Sony%20-%20PlayStation%202/Named_Boxarts/{encoded}"),
                    format!("https://raw.githubusercontent.com/libretro-thumbnails/Sony_-_PlayStation_2/master/Named_Boxarts/{encoded}"),
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
}
