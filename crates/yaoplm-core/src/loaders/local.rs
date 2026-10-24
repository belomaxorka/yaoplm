//! A loader file the user already has: an ELF, or a zip or 7z with one
//! (SPEC LDR-2, install without the network). Nothing vouches for it, so
//! YAOPLM says what the file itself says: its family and version inside
//! the ELF (ADR-0041), the catalogue channel that version belongs to, and
//! the frozen release its SHA-256 matches, if any (ADR-0042).

use std::io::Cursor;

use yaoplm_formats::archive;
use yaoplm_formats::ps2_elf;
use yaoplm_i18n::Phrase;

use super::download::{self, LoaderElf};
use super::version::BuildVersion;
use super::{Catalog, LoaderError, catalog::glob_match};
use crate::detect::{self, Family};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalLoader {
    pub elf: LoaderElf,
    /// The archive member the ELF came from.
    pub member: Option<String>,
    /// From the program's own strings.
    pub family: Option<Family>,
    /// The catalogue build its version belongs to.
    pub build: Option<String>,
    /// The frozen upstream release its SHA-256 is (`opl-stable.txt`).
    pub stable: Option<&'static str>,
}

/// Names of loader ELFs any catalogue build uses (`fingerprints.elf`).
fn loader_name(catalog: &Catalog, name: &str) -> bool {
    let file = name.rsplit('/').next().unwrap_or(name);
    catalog
        .builds
        .iter()
        .flat_map(|b| &b.fingerprints.elf)
        .any(|pattern| glob_match(pattern, file, true))
}

/// Reads the file `name`; `bytes` is all of it, at most
/// [`download::MAX_FILE`].
pub fn read(catalog: &Catalog, name: &str, bytes: &[u8]) -> Result<LocalLoader, LoaderError> {
    let no_elf = || LoaderError::new(Phrase::error("loaderFileNoElf").with("file", name));
    let (elf_bytes, member) = if bytes.starts_with(b"\x7FELF") {
        (bytes.to_vec(), None)
    } else {
        let kind = download::archive_kind(name, bytes).ok_or_else(no_elf)?;
        let entries = archive::list(kind, &mut Cursor::new(bytes)).map_err(|e| {
            LoaderError::new(
                Phrase::error("loaderArchiveUnreadable")
                    .with("archive", name)
                    .with("error", e.to_string()),
            )
        })?;
        let elfs: Vec<String> = entries
            .into_iter()
            .filter(|e| !e.is_dir && loader_name(catalog, &e.name))
            .map(|e| e.name)
            .collect();
        match elfs.as_slice() {
            [] => return Err(no_elf()),
            [one] => (download::extract(name, bytes, one)?, Some(one.clone())),
            several => {
                return Err(LoaderError::new(
                    Phrase::error("loaderFileSeveralElfs")
                        .with("file", name)
                        .with("count", several.len() as u64),
                ));
            }
        }
    };
    let shown = member.as_deref().unwrap_or(name);
    let family = ps2_elf::program(&elf_bytes)
        .ok()
        .and_then(|program| detect::program_family(&program));
    let elf = download::elf(shown, elf_bytes, None)?;
    let build = elf
        .version
        .as_deref()
        .and_then(BuildVersion::parse)
        .and_then(|version| catalog.channel_of_version(family.unwrap_or(Family::Opl), &version))
        .map(|b| b.id.clone());
    Ok(LocalLoader {
        stable: detect::stable_version(&elf.sha256),
        elf,
        member,
        family,
        build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loaders::built_in;
    use crate::loaders::download::tests::{plain_elf, zip};

    #[test]
    fn an_elf_says_what_it_is() {
        let catalog = built_in().unwrap();
        let elf = plain_elf(&["conf_opl.cfg", "v1.2.0-Beta-2370-33a2b19-yaopl"]);
        let got = read(catalog, "OPL.ELF", &elf).unwrap();
        assert_eq!(got.family, Some(Family::Opl));
        assert_eq!(got.build.as_deref(), Some("yaopl"));
        assert_eq!(
            got.elf.version.as_deref(),
            Some("v1.2.0-Beta-2370-33a2b19-yaopl")
        );
        assert_eq!(got.stable, None);
        assert_eq!(got.member, None);

        // An old beta from the MEGA archive is still a beta of the channel.
        let old = plain_elf(&["conf_opl.cfg", "v1.2.0-Beta-1990-0123abc"]);
        assert_eq!(
            read(catalog, "x.elf", &old).unwrap().build.as_deref(),
            Some("opl-beta")
        );
        // 1.0.0 has no channel; another family is not OPL's.
        let v100 = plain_elf(&["conf_opl.cfg", "v1.0.0"]);
        assert_eq!(read(catalog, "x.elf", &v100).unwrap().build, None);
        let ript = plain_elf(&[
            "settings_riptopl.cfg",
            "conf_opl.cfg",
            "v1.2.0-Beta-4257-55261de",
        ]);
        let got = read(catalog, "RIPTOPL.ELF", &ript).unwrap();
        assert_eq!((got.family, got.build), (Some(Family::RiptOpl), None));
    }

    #[test]
    fn an_archive_gives_its_one_loader() {
        let catalog = built_in().unwrap();
        let elf = plain_elf(&["conf_opl.cfg", "v1.1.0"]);
        let one = zip(&[
            ("OPNPS2LD/README.md", b"readme".as_slice()),
            ("OPNPS2LD/OPNPS2LD-v1.1.0.ELF", &elf),
        ]);
        let got = read(catalog, "OPNPS2LD-v1.1.0.zip", &one).unwrap();
        assert_eq!(got.member.as_deref(), Some("OPNPS2LD/OPNPS2LD-v1.1.0.ELF"));
        assert_eq!(got.build.as_deref(), Some("opl-stable"));
        assert_eq!(got.elf.bytes, elf);

        let two = zip(&[("A/OPNPS2LD.ELF", elf.as_slice()), ("B/OPL.ELF", &elf)]);
        assert_eq!(
            read(catalog, "two.zip", &two).unwrap_err().phrase.key,
            "errors.loaderFileSeveralElfs"
        );
        let none = zip(&[("README.md", b"readme".as_slice())]);
        assert_eq!(
            read(catalog, "none.zip", &none).unwrap_err().phrase.key,
            "errors.loaderFileNoElf"
        );
        assert_eq!(
            read(catalog, "notes.txt", b"hello").unwrap_err().phrase.key,
            "errors.loaderFileNoElf"
        );
        let bad = b"\x7FELF\x02\x02 not mips".to_vec();
        assert_eq!(
            read(catalog, "x.elf", &bad).unwrap_err().phrase.key,
            "errors.loaderNotPs2Elf"
        );
    }
}
