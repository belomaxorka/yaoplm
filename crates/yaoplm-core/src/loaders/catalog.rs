//! Entries of `data/loaders/<id>.toml` (SPEC §5.10.1, ADR-0038, ADR-0042).
//! Parsing is strict: an unknown field is an error, so a typo in the data
//! fails the tests instead of silently dropping a rule. New fields bump
//! `schema` in `index.toml`.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};
use yaoplm_device::RelPath;
use yaoplm_formats::LoaderProfile;

use super::version::BuildVersion;
use super::{CatalogError, SCHEMA};
use crate::detect::Family;

/// Texts by interface language (`en`, `ru`): the catalogue changes without
/// a new program, so build descriptions live here, not in the UI catalogues.
pub type Texts = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub schema: u32,
    /// In the order of `index.toml`.
    pub builds: Vec<Build>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: u32,
    builds: Vec<String>,
}

/// One build: a family and a channel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// The file name without `.toml`.
    pub id: String,
    pub family: Family,
    pub name: String,
    pub publisher: Publisher,
    /// `owner/name` on GitHub.
    pub repo: String,
    /// Format rules of the build (SPEC §4).
    #[serde(deserialize_with = "profile")]
    pub profile: LoaderProfile,
    /// The one build the build cards mark "Recommended" (LDR-6).
    #[serde(default)]
    pub recommended: bool,
    pub devices: Vec<Target>,
    pub license: License,
    /// What the build is good at, for the build cards.
    pub summary: Texts,
    /// What it cannot do.
    pub limits: Texts,
    #[serde(default)]
    pub docs: Vec<Doc>,
    pub channel: Channel,
    pub layout: Layout,
    pub fingerprints: Fingerprints,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Publisher {
    /// The project's own releases.
    Official,
    Fork,
}

/// Where a build can run from (SPEC §4 "Носители").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Usb,
    /// An SD card in the MX4SIO adapter.
    Mx4sio,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    pub spdx: String,
    pub url: String,
    pub authors: String,
}

/// Official documentation the device card and the wizard link to; YAOPLM
/// does not retell it (SPEC §5.10.1).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doc {
    pub kind: DocKind,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Readme,
    Site,
    Wiki,
    /// What a fork changes.
    Fork,
}

/// How to find the release of the channel and its files.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub tag: String,
    /// The tag is re-published in place (`latest`): pin by hash, never by
    /// tag (LDR-2).
    pub floating: bool,
    pub prerelease: bool,
    /// Release tags of the repository that belong to this channel, `*`
    /// for any text: what "Other version…" offers (LDR-6).
    pub releases: Vec<String>,
    pub version: VersionRule,
    pub elf: ElfSource,
    /// The language pack; `{version}` is the build's version.
    #[serde(default)]
    pub languages: Option<String>,
    pub checksums: Checksums,
}

/// Where the version string of a release comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionRule {
    /// The tag is the version (`v1.1.0`).
    Tag,
    /// The release's name is (YAOPL).
    Name,
    /// An asset name with `{version}` in it (`OPNPS2LD-{version}.7z`).
    Asset(String),
}

impl<'de> Deserialize<'de> for VersionRule {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d)?.as_str() {
            "tag" => Self::Tag,
            "name" => Self::Name,
            template => Self::Asset(template.to_owned()),
        })
    }
}

impl VersionRule {
    /// The version in an asset's name, when the rule names assets.
    pub fn from_asset(&self, asset: &str) -> Option<BuildVersion> {
        match self {
            Self::Asset(template) => from_template(template, asset),
            Self::Tag | Self::Name => None,
        }
    }
}

/// The version a name carries in the place of `{version}`.
pub fn from_template(template: &str, name: &str) -> Option<BuildVersion> {
    let (prefix, suffix) = template.split_once("{version}")?;
    let middle = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
    BuildVersion::parse(middle)
}

/// The ELF of a release.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum ElfSource {
    /// An asset of its own.
    Asset(String),
    /// A member of an archive asset (`/` between folders).
    Member { archive: String, member: String },
}

impl ElfSource {
    /// The asset to download.
    pub fn asset(&self) -> &str {
        match self {
            Self::Asset(name) | Self::Member { archive: name, .. } => name,
        }
    }
}

/// How a downloaded file is confirmed (LDR-2). A file is accepted only
/// when every source that knows it agrees.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checksums {
    /// GitHub's `digest` of each asset.
    #[serde(default)]
    pub digest: bool,
    /// An asset of `sha256sum` lines.
    #[serde(default)]
    pub sums: Option<String>,
    /// SHA-256 by asset name, for releases GitHub has no digest for.
    #[serde(default)]
    pub pinned: BTreeMap<String, String>,
}

/// What a build needs on the device.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    /// Folders the build reads (LIB-3).
    pub folders: Vec<String>,
    /// The global settings file.
    pub settings: String,
    pub settings_location: SettingsLocation,
    /// Keys starter settings may set (SPEC §5.10.1, ADR-0040).
    #[serde(default)]
    pub seed: Vec<String>,
    /// The folder language files go to.
    #[serde(default)]
    pub languages: Option<String>,
    /// The preset a wizard picks for each device.
    pub defaults: BTreeMap<Target, String>,
    pub presets: Vec<Preset>,
}

/// Where the loader looks for its settings first (research/07 §3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettingsLocation {
    /// The memory card, then the device it started from: a file on the
    /// device counts only while the card has none (OPL, YAOPL).
    MemoryCard,
    /// The ELF's folder (RiptOPL, wOPL).
    BesideElf,
}

/// Where the ELF goes for a boot method (research/07 §2.9, research/17
/// §9 a).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    /// Relative to the device root.
    pub path: String,
    /// An app folder: the `title=` of the `title.cfg` beside the ELF.
    #[serde(default)]
    pub title: Option<String>,
    pub devices: Vec<Target>,
    /// Launchers that start the ELF there, for the boot hints (BOOT-1).
    pub launchers: Vec<Launcher>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Launcher {
    /// FreeMcBoot 1.953–1.966, its stock `FREEMCB.CNF`.
    Fmcb,
    /// PS2BBL with its stock `PS2BBL.INI`.
    Ps2bbl,
    /// PS2BBL's MX4SIO build: `massX:` paths.
    Ps2bblMx4sio,
    /// OSDMenu: any path in `OSDMENU.CNF`.
    Osdmenu,
    /// OPL's own Apps page, from another copy of OPL.
    OplApps,
}

/// What the build leaves on a device (research/16).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprints {
    /// ELF names, `*` for any text, case does not matter.
    pub elf: Vec<String>,
    pub settings: Vec<String>,
    /// Version strings of this channel, `*` for any text: the version
    /// inside an ELF names its channel (ADR-0041, ADR-0042).
    pub versions: Vec<String>,
    /// What a fork appends after the commit (`yaopl`); without it the
    /// version must have no suffix at all.
    #[serde(default)]
    pub suffix: Option<String>,
}

fn profile<'de, D: Deserializer<'de>>(d: D) -> Result<LoaderProfile, D::Error> {
    let name = String::deserialize(d)?;
    match name.as_str() {
        "opl-1.2" => Ok(LoaderProfile::Opl120Beta),
        "opl-1.1" => Ok(LoaderProfile::Opl110),
        "riptopl" => Ok(LoaderProfile::RiptOpl),
        other => Err(serde::de::Error::custom(format!("unknown profile {other}"))),
    }
}

fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// `*` matches any text, nothing else is special.
pub fn glob_match(pattern: &str, text: &str, ignore_case: bool) -> bool {
    let fold = |s: &str| {
        if ignore_case {
            s.to_ascii_lowercase()
        } else {
            s.to_owned()
        }
    };
    let (pattern, text) = (fold(pattern), fold(text));
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

impl Catalog {
    /// Reads a catalogue through `read`, which gets paths under `data/`
    /// (`loaders/index.toml`, `loaders/<id>.toml`).
    pub fn parse(mut read: impl FnMut(&str) -> Option<String>) -> Result<Self, CatalogError> {
        const INDEX: &str = "loaders/index.toml";
        let text = read(INDEX).ok_or_else(|| CatalogError::new(INDEX, "missing"))?;
        let index: Index =
            toml::from_str(&text).map_err(|e| CatalogError::new(INDEX, e.to_string()))?;
        if index.schema == 0 || index.schema > SCHEMA {
            return Err(CatalogError::new(
                INDEX,
                format!(
                    "schema {} is not one this program reads (up to {SCHEMA})",
                    index.schema
                ),
            ));
        }
        let mut builds: Vec<Build> = Vec::new();
        for id in &index.builds {
            let file = format!("loaders/{id}.toml");
            if builds.iter().any(|b| &b.id == id) {
                return Err(CatalogError::new(INDEX, format!("{id} is listed twice")));
            }
            let text = read(&file).ok_or_else(|| CatalogError::new(&file, "missing"))?;
            let build: Build =
                toml::from_str(&text).map_err(|e| CatalogError::new(&file, e.to_string()))?;
            if &build.id != id {
                return Err(CatalogError::new(
                    &file,
                    format!("id is {}, not {id}", build.id),
                ));
            }
            build
                .check()
                .map_err(|message| CatalogError::new(&file, message))?;
            builds.push(build);
        }
        if builds.iter().filter(|b| b.recommended).count() != 1 {
            return Err(CatalogError::new(
                INDEX,
                "exactly one build must be recommended",
            ));
        }
        Ok(Self {
            schema: index.schema,
            builds,
        })
    }

    pub fn build(&self, id: &str) -> Option<&Build> {
        self.builds.iter().find(|b| b.id == id)
    }

    pub fn recommended(&self) -> Option<&Build> {
        self.builds.iter().find(|b| b.recommended)
    }

    /// The build a release of `repo` belongs to (LDR-6): releases outside
    /// every channel are not offered.
    pub fn channel_of(&self, repo: &str, tag: &str) -> Option<&Build> {
        self.builds.iter().find(|b| {
            b.repo.eq_ignore_ascii_case(repo)
                && b.channel.releases.iter().any(|p| glob_match(p, tag, false))
        })
    }

    /// The channel a build of `family` with this version belongs to: what
    /// the version inside an ELF says (LDR-4). `None` for versions no
    /// channel takes, such as OPL 1.0.0.
    pub fn channel_of_version(&self, family: Family, version: &BuildVersion) -> Option<&Build> {
        self.builds
            .iter()
            .find(|b| b.family == family && b.takes_version(version))
    }
}

impl Build {
    pub fn preset(&self, id: &str) -> Option<&Preset> {
        self.layout.presets.iter().find(|p| p.id == id)
    }

    /// The preset a wizard starts with for `target`.
    pub fn default_preset(&self, target: Target) -> Option<&Preset> {
        self.preset(self.layout.defaults.get(&target)?)
    }

    /// The version is one of this channel's: a pattern of
    /// `fingerprints.versions` and exactly the channel's fork suffix.
    pub fn takes_version(&self, version: &BuildVersion) -> bool {
        let suffix_fits = match &self.fingerprints.suffix {
            Some(suffix) => version.suffix.first() == Some(suffix),
            None => version.suffix.is_empty(),
        };
        let text = version.to_string();
        suffix_fits
            && self
                .fingerprints
                .versions
                .iter()
                .any(|p| glob_match(p, &text, false))
    }

    /// Rules the data must keep; the message names the field.
    fn check(&self) -> Result<(), String> {
        let ascii_name = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        };
        if !ascii_name(&self.id) {
            return Err("id must be a plain ASCII name".into());
        }
        if self.name.trim().is_empty() {
            return Err("name is empty".into());
        }
        let mut repo = self.repo.split('/');
        if !(repo.next().is_some_and(ascii_name)
            && repo.next().is_some_and(ascii_name)
            && repo.next().is_none())
        {
            return Err(format!("repo {} is not owner/name", self.repo));
        }
        let profile_fits = match self.family {
            Family::Opl => matches!(
                self.profile,
                LoaderProfile::Opl110 | LoaderProfile::Opl120Beta
            ),
            Family::RiptOpl => self.profile == LoaderProfile::RiptOpl,
            Family::Wopl | Family::Uopl | Family::Nhddl => {
                self.profile == LoaderProfile::Opl120Beta
            }
        };
        if !profile_fits {
            return Err("profile does not fit the family".into());
        }
        if self.devices.is_empty() {
            return Err("devices is empty".into());
        }
        for (field, texts) in [("summary", &self.summary), ("limits", &self.limits)] {
            if !texts.contains_key("en") {
                return Err(format!("{field} has no en text"));
            }
            if let Some((lang, _)) = texts
                .iter()
                .find(|(_, t)| t.trim().is_empty() || t.trim() != t.as_str())
            {
                return Err(format!("{field}.{lang} is empty or has spaces at its ends"));
            }
        }
        let https = |url: &str| url.starts_with("https://") && !url.contains(char::is_whitespace);
        if !https(&self.license.url) || self.docs.iter().any(|d| !https(&d.url)) {
            return Err("links must be https".into());
        }
        if self.license.spdx.is_empty() || self.license.authors.trim().is_empty() {
            return Err("license needs spdx and authors".into());
        }
        self.check_channel()?;
        self.check_layout()?;
        if self.fingerprints.elf.is_empty() {
            return Err("fingerprints.elf is empty".into());
        }
        let versions = &self.fingerprints.versions;
        if versions.is_empty() || versions.iter().any(|p| !p.starts_with('v')) {
            return Err("fingerprints.versions must be version patterns".into());
        }
        Ok(())
    }

    fn check_channel(&self) -> Result<(), String> {
        let channel = &self.channel;
        if !channel
            .releases
            .iter()
            .any(|p| glob_match(p, &channel.tag, false))
        {
            return Err("channel.releases must take channel.tag".into());
        }
        if let VersionRule::Asset(template) = &channel.version
            && template.matches("{version}").count() != 1
        {
            return Err("channel.version needs one {version}".into());
        }
        if let Some(languages) = &channel.languages
            && languages.matches("{version}").count() > 1
        {
            return Err("channel.languages has more than one {version}".into());
        }
        if let ElfSource::Member { member, .. } = &channel.elf
            && (RelPath::new(member).is_err() || member.is_empty())
        {
            return Err(format!(
                "channel.elf member {member} is not a relative path"
            ));
        }
        let sums = &channel.checksums;
        if let Some((name, sha)) = sums.pinned.iter().find(|(_, sha)| !is_sha256(sha)) {
            return Err(format!(
                "channel.checksums.pinned {name} = {sha} is not a lowercase SHA-256"
            ));
        }
        if !sums.digest && sums.sums.is_none() {
            // Nothing in the release confirms its files: each must be pinned.
            let fixed = channel
                .languages
                .iter()
                .filter(|l| !l.contains("{version}"));
            for asset in std::iter::once(channel.elf.asset()).chain(fixed.map(String::as_str)) {
                if !sums.pinned.contains_key(asset) {
                    return Err(format!("channel.checksums: nothing confirms {asset}"));
                }
            }
        }
        Ok(())
    }

    fn check_layout(&self) -> Result<(), String> {
        let layout = &self.layout;
        if layout.folders.is_empty()
            || layout
                .folders
                .iter()
                .any(|f| RelPath::new(f).is_err() || f.is_empty())
        {
            return Err("layout.folders must be folder names".into());
        }
        if let Some(languages) = &layout.languages
            && !layout.folders.contains(languages)
        {
            return Err("layout.languages must be one of layout.folders".into());
        }
        if layout.presets.is_empty() {
            return Err("layout.presets is empty".into());
        }
        for (index, preset) in layout.presets.iter().enumerate() {
            let name = &preset.id;
            if layout.presets[..index].iter().any(|p| &p.id == name) {
                return Err(format!("preset {name} is defined twice"));
            }
            let path = RelPath::new(&preset.path).map_err(|e| format!("preset {name}: {e}"))?;
            if path.as_str().is_empty() || !preset.path.to_ascii_uppercase().ends_with(".ELF") {
                return Err(format!("preset {name}: the path must end in .ELF"));
            }
            if preset.title.is_some() && !preset.path.contains('/') {
                return Err(format!("preset {name}: a title needs an app folder"));
            }
            if preset.devices.is_empty() || preset.devices.iter().any(|d| !self.devices.contains(d))
            {
                return Err(format!(
                    "preset {name}: devices must be some of the build's"
                ));
            }
            if preset.launchers.is_empty() {
                return Err(format!("preset {name}: no launcher"));
            }
        }
        for device in &self.devices {
            let Some(id) = layout.defaults.get(device) else {
                return Err(format!("layout.defaults has no preset for {device:?}"));
            };
            match self.preset(id) {
                Some(preset) if preset.devices.contains(device) => {}
                _ => {
                    return Err(format!(
                        "layout.defaults: {id} is not a preset for {device:?}"
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;
    use crate::loaders::version::BuildVersion;
    use crate::loaders::{BUILT_IN, built_in};

    fn data_dir() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"))
    }

    fn files_in(dir: &str, extension: &str) -> BTreeSet<String> {
        std::fs::read_dir(data_dir().join(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(extension))
            .map(|name| format!("{dir}/{name}"))
            .collect()
    }

    /// The interface languages: `web/src/locales/<lang>/`.
    fn languages() -> BTreeSet<String> {
        std::fs::read_dir(data_dir().join("../web/src/locales"))
            .unwrap()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().unwrap().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_built_in_catalogue_reads() {
        let catalog = built_in().unwrap();
        let ids: Vec<&str> = catalog.builds.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, ["opl-stable", "opl-beta", "yaopl"]);
        assert_eq!(catalog.recommended().unwrap().id, "opl-beta");
    }

    #[test]
    fn every_data_file_is_compiled_in() {
        let on_disk = files_in("loaders", ".toml");
        let compiled: BTreeSet<String> = BUILT_IN.iter().map(|(p, _)| (*p).to_owned()).collect();
        assert_eq!(on_disk, compiled);
        for (path, text) in BUILT_IN {
            let disk = std::fs::read_to_string(data_dir().join(path)).unwrap();
            assert_eq!(disk, text, "{path}");
        }
        // The only hash table is the frozen one detection reads (ADR-0042).
        assert_eq!(
            files_in("loader-hashes", ".txt"),
            BTreeSet::from(["loader-hashes/opl-stable.txt".to_owned()])
        );
    }

    #[test]
    fn texts_exist_in_every_interface_language() {
        let languages = languages();
        assert!(languages.contains("en") && languages.contains("ru"));
        for build in &built_in().unwrap().builds {
            for (field, texts) in [("summary", &build.summary), ("limits", &build.limits)] {
                let have: BTreeSet<String> = texts.keys().cloned().collect();
                assert_eq!(have, languages, "{}: {field}", build.id);
            }
        }
    }

    /// docs/I18N.md rule 5, as the web catalogue test checks it.
    #[test]
    fn texts_follow_the_typography_rules() {
        for build in &built_in().unwrap().builds {
            for (lang, text) in build.summary.iter().chain(&build.limits) {
                let at = format!("{} {lang}: {text}", build.id);
                assert!(!text.contains("..."), "{at}");
                assert!(!text.contains('"') && !text.contains('\''), "{at}");
                assert!(!text.contains("  "), "{at}");
                assert!(!text.contains(" ,") && !text.contains(" ."), "{at}");
                assert!(text.ends_with('.'), "{at}");
                if lang == "ru" {
                    assert!(!text.contains(" - "), "{at}: a dash, not a hyphen");
                }
            }
        }
    }

    #[test]
    fn opl_builds_create_the_folders_opl_creates() {
        let opl: BTreeSet<&str> = crate::scan::OPL_FOLDERS.into_iter().collect();
        for build in built_in()
            .unwrap()
            .builds
            .iter()
            .filter(|b| b.family == Family::Opl)
        {
            let folders: BTreeSet<&str> = build.layout.folders.iter().map(String::as_str).collect();
            assert_eq!(folders, opl, "{}", build.id);
            assert_eq!(build.layout.languages.as_deref(), Some("LNG"));
        }
    }

    #[test]
    fn presets_follow_the_owner_decisions() {
        // ADR-0042: FreeMcBoot's UPDT/OPL.ELF on a stick, an app folder on
        // MX4SIO, PS2BBL's APPS/OPNPS2LD.ELF beside them.
        for build in &built_in().unwrap().builds {
            let usb = build.default_preset(Target::Usb).unwrap();
            assert_eq!(
                (usb.id.as_str(), usb.path.as_str()),
                ("fmcb", "UPDT/OPL.ELF"),
                "{}",
                build.id
            );
            let mx4sio = build.default_preset(Target::Mx4sio).unwrap();
            assert_eq!(mx4sio.id, "app");
            assert!(mx4sio.title.is_some() && mx4sio.path.starts_with("APPS/"));
            assert_eq!(build.preset("ps2bbl").unwrap().path, "APPS/OPNPS2LD.ELF");
        }
    }

    #[test]
    fn channels_take_only_their_releases() {
        let catalog = built_in().unwrap();
        let upstream = "ps2homebrew/Open-PS2-Loader";
        assert_eq!(
            catalog.channel_of(upstream, "latest").unwrap().id,
            "opl-beta"
        );
        assert_eq!(
            catalog.channel_of(upstream, "v1.1.0").unwrap().id,
            "opl-stable"
        );
        // Older releases have no profile (SPEC §5.10.2).
        for tag in ["v1.0.0", "v1.0.0-rc1", "0.9.3"] {
            assert!(catalog.channel_of(upstream, tag).is_none(), "{tag}");
        }
        let fork = "belomaxorka/yaopl";
        for tag in ["latest", "yaopl-v2.0.0", "yaopl-v2.0.0-rc1"] {
            assert_eq!(catalog.channel_of(fork, tag).unwrap().id, "yaopl", "{tag}");
        }
        // Upstream tags inherited through git are not YAOPL releases.
        for tag in ["v1.1.0", "v1.0.0-rc1"] {
            assert!(catalog.channel_of(fork, tag).is_none(), "{tag}");
        }
    }

    #[test]
    fn the_version_inside_an_elf_names_its_channel() {
        let catalog = built_in().unwrap();
        let channel = |text: &str| {
            catalog
                .channel_of_version(Family::Opl, &BuildVersion::parse(text).unwrap())
                .map(|b| b.id.as_str())
        };
        // The three ELFs downloaded for the catalogue (research/17 §7.3).
        assert_eq!(channel("v1.1.0"), Some("opl-stable"));
        assert_eq!(channel("v1.2.0-Beta-2271-b3a2199"), Some("opl-beta"));
        assert_eq!(channel("v1.2.0-Beta-2370-33a2b19-yaopl"), Some("yaopl"));
        // YAOPL's A/B build is still YAOPL; another fork is no channel.
        assert_eq!(
            channel("v1.2.0-Beta-2370-33a2b19-yaopl-nosio2man"),
            Some("yaopl")
        );
        assert_eq!(channel("v1.2.0-Beta-4257-55261de-riptopl"), None);
        // YAOPL's own versions (ADR-0051), the A/B build among them.
        for text in [
            "v2.0.0-nightly.12-0123abc-yaopl",
            "v2.0.0-nightly.12-0123abc-yaopl-nosio2man",
            "v2.0.0-rc1-yaopl",
            "v2.0.0-yaopl",
            "v2.1.0-nightly.1-0123abc-yaopl",
        ] {
            assert_eq!(channel(text), Some("yaopl"), "{text}");
        }
        // A local build is none of the published ones.
        assert_eq!(channel("v2.0.0-nightly.12-0123abc-dirty-yaopl"), None);
        assert_eq!(channel("v2.0.0-nightly.12-0123abc"), None);
        // Releases without a profile have no channel either (SPEC §5.10.2).
        assert_eq!(channel("v1.0.0"), None);
        assert_eq!(channel("v0.9.3"), None);
        let version = BuildVersion::parse("v1.2.0-Beta-2271-b3a2199").unwrap();
        assert!(
            catalog
                .channel_of_version(Family::RiptOpl, &version)
                .is_none()
        );
    }

    #[test]
    fn every_channel_takes_its_own_releases_versions() {
        let catalog = built_in().unwrap();
        for build in &catalog.builds {
            if let VersionRule::Tag = build.channel.version {
                let version = BuildVersion::parse(&build.channel.tag).unwrap();
                assert!(build.takes_version(&version), "{}", build.id);
            }
            // Channels never overlap: a version names one channel at most.
            for other in catalog.builds.iter().filter(|o| o.id != build.id) {
                for pattern in &build.fingerprints.versions {
                    // What `*` stands for in the patterns: a beta's number
                    // and commit, a whole version.
                    let samples: Vec<BuildVersion> = ["2271-b3a2199", "2.0.0-nightly.3-b3a2199"]
                        .iter()
                        .map(|fill| pattern.replace('*', fill))
                        .map(|sample| match &build.fingerprints.suffix {
                            Some(suffix) => format!("{sample}-{suffix}"),
                            None => sample,
                        })
                        .filter_map(|sample| BuildVersion::parse(&sample))
                        .collect();
                    assert!(!samples.is_empty(), "{}: {pattern}", build.id);
                    for version in samples {
                        let sample = version.to_string();
                        assert!(build.takes_version(&version), "{}: {sample}", build.id);
                        assert!(
                            !other.takes_version(&version),
                            "{} and {}: {sample}",
                            build.id,
                            other.id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn versions_come_from_asset_names() {
        let beta = &built_in()
            .unwrap()
            .build("opl-beta")
            .unwrap()
            .channel
            .version;
        let v = beta
            .from_asset("OPNPS2LD-v1.2.0-Beta-2271-b3a2199.7z")
            .unwrap();
        assert_eq!(v.to_string(), "v1.2.0-Beta-2271-b3a2199");
        for other in [
            "OPNPS2LD.7z",
            "OPNPS2LD-VARIANTS-v1.2.0-Beta-2271-b3a2199.7z",
            "OPNPS2LD-LANGS.7z",
        ] {
            assert!(beta.from_asset(other).is_none(), "{other}");
        }
        assert!(VersionRule::Tag.from_asset("OPNPS2LD-v1.1.0.7z").is_none());
    }

    #[test]
    fn globs() {
        assert!(glob_match("OPNPS2LD*.ELF", "opnps2ld-v1.1.0.elf", true));
        assert!(glob_match("OPNPS2LD*.ELF", "OPNPS2LD.ELF", true));
        assert!(!glob_match("OPNPS2LD*.ELF", "OPNPS2LD.ELF.bak", true));
        assert!(glob_match("v*-yaopl", "v1.2.0-Beta-1-0123abc-yaopl", false));
        assert!(!glob_match("v*-yaopl", "V1-yaopl", false));
        assert!(glob_match("latest", "latest", false));
        assert!(!glob_match("latest", "latest2", false));
        assert!(glob_match("a*b*c", "aXbYc", false));
        assert!(!glob_match("a*bc", "abc*", false));
        assert!(glob_match("*", "", false));
    }

    fn catalog_with(build: &str) -> Result<Catalog, CatalogError> {
        Catalog::parse(|path| match path {
            "loaders/index.toml" => Some("schema = 1\nbuilds = [\"x\"]\n".to_owned()),
            "loaders/x.toml" => Some(build.to_owned()),
            _ => None,
        })
    }

    /// The YAOPL entry renamed, so that each case breaks one rule.
    fn base() -> String {
        BUILT_IN
            .iter()
            .find(|(p, _)| *p == "loaders/yaopl.toml")
            .unwrap()
            .1
            .replace("id = \"yaopl\"", "id = \"x\"")
            .replace("recommended = false", "recommended = true")
    }

    #[test]
    fn a_base_entry_reads() {
        let catalog = catalog_with(&base()).unwrap();
        assert_eq!(catalog.builds[0].id, "x");
    }

    #[test]
    fn broken_entries_are_refused() {
        let cases = [
            (
                "name = \"YAOPL\"",
                "name = \"YAOPL\"\ncolour = \"red\"",
                "unknown field",
            ),
            (
                "profile = \"opl-1.2\"",
                "profile = \"opl-9\"",
                "unknown profile",
            ),
            ("profile = \"opl-1.2\"", "profile = \"riptopl\"", "family"),
            (
                "repo = \"belomaxorka/yaopl\"",
                "repo = \"yaopl\"",
                "owner/name",
            ),
            ("recommended = true", "recommended = false", "recommended"),
            ("versions = [\"v*\"]\n", "", "missing field `versions`"),
            (
                "releases = [\"latest\", \"yaopl-v*\"]",
                "releases = [\"yaopl-v*\"]",
                "channel.tag",
            ),
            (
                "path = \"UPDT/OPL.ELF\"",
                "path = \"../OPL.ELF\"",
                "preset fmcb",
            ),
            ("path = \"UPDT/OPL.ELF\"", "path = \"UPDT/OPL.BIN\"", ".ELF"),
            ("mx4sio = \"app\"", "mx4sio = \"fmcb\"", "layout.defaults"),
            (
                "checksums = { digest = true, sums = \"SHA256SUMS\" }",
                "checksums = {}",
                "nothing confirms",
            ),
            (
                "url = \"https://github.com/belomaxorka/yaopl/blob/master/LICENSE\"",
                "url = \"http://example.org\"",
                "https",
            ),
            (
                "en = \"Unofficial build.\"",
                "de = \"Unofficial build.\"",
                "no en text",
            ),
        ];
        for (from, to, expect) in cases {
            let text = base();
            assert!(text.contains(from), "{from}");
            let err = catalog_with(&text.replacen(from, to, 1)).unwrap_err();
            assert!(err.to_string().contains(expect), "{from} -> {to}: {err}");
        }
    }

    #[test]
    fn a_newer_schema_keeps_the_own_copy() {
        let err = Catalog::parse(|path| {
            (path == "loaders/index.toml").then(|| "schema = 2\nbuilds = []\n".to_owned())
        })
        .unwrap_err();
        assert!(err.to_string().contains("schema 2"));
    }
}
