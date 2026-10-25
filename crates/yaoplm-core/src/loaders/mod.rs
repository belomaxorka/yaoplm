//! The loader catalogue (SPEC LDR-1, §5.10.1, ADR-0022, ADR-0042): the
//! builds YAOPLM installs, as data in `data/loaders/`. There is no hash
//! history of new builds: the version inside an ELF (ADR-0041) names the
//! build and, through `fingerprints`, its channel.
//!
//! A build is a family plus a channel: OPL 1.1.0 (`v1.1.0`), OPL 1.2.0 beta
//! (`latest`) and YAOPL (`latest`) are three builds of the OPL family. A copy
//! of the catalogue is compiled in; a newer one from `main` goes through the
//! same parser and checks, so a broken file can never replace a good copy.
//! Release facts the entries rely on are in research/17 §6–§7.

pub mod boot;
pub mod catalog;
pub mod download;
pub mod install;
pub mod installed;
pub mod languages;
pub mod local;
pub mod manifest;
pub mod notes;
pub mod release;
pub mod rollback;
pub mod version;

use std::sync::OnceLock;

pub use catalog::{
    Build, Catalog, Channel, Checksums, Doc, DocKind, ElfSource, Launcher, Layout, License, Preset,
    Publisher, SettingsLocation, Target, VersionRule,
};
pub use version::BuildVersion;

use yaoplm_i18n::Phrase;

/// The newest catalogue schema this program reads.
pub const SCHEMA: u32 = 1;

/// A file of the catalogue that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}: {message}")]
pub struct CatalogError {
    /// Relative to `data/`: `loaders/opl-beta.toml`.
    pub file: String,
    pub message: String,
}

impl CatalogError {
    pub(crate) fn new(file: &str, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            message: message.into(),
        }
    }
}

/// Why a release, a download or a user's file cannot be used, said to the
/// user (`errors.loader*`, ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .phrase.english())]
pub struct LoaderError {
    pub phrase: Phrase,
}

impl LoaderError {
    pub(crate) fn new(phrase: Phrase) -> Self {
        Self { phrase }
    }
}

/// The catalogue files compiled into the program, by their path under
/// `data/`. `catalog::tests` checks that this is every file there.
const BUILT_IN: [(&str, &str); 4] = [
    (
        "loaders/index.toml",
        include_str!("../../../../data/loaders/index.toml"),
    ),
    (
        "loaders/opl-stable.toml",
        include_str!("../../../../data/loaders/opl-stable.toml"),
    ),
    (
        "loaders/opl-beta.toml",
        include_str!("../../../../data/loaders/opl-beta.toml"),
    ),
    (
        "loaders/yaopl.toml",
        include_str!("../../../../data/loaders/yaopl.toml"),
    ),
];

/// The catalogue compiled into this program. An error means a broken data
/// file slipped past the tests.
pub fn built_in() -> Result<&'static Catalog, CatalogError> {
    static CATALOG: OnceLock<Result<Catalog, CatalogError>> = OnceLock::new();
    CATALOG
        .get_or_init(|| {
            Catalog::parse(|path| {
                BUILT_IN
                    .iter()
                    .find(|(name, _)| *name == path)
                    .map(|(_, text)| (*text).to_owned())
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}
