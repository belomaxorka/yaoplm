//! What is installed on a device, as the catalogue knows it (SPEC LDR-4,
//! §5.10.1, ADR-0040, ADR-0045): every loader the detection found, plus the
//! manifest's entries at paths the detection does not look at, each with
//! its catalogue build, how that is known, the manifest's word and the
//! newest backup of its path. The manifest never decides alone: it counts
//! only while its SHA-256 is the file's; otherwise the file was replaced
//! without YAOPLM and the file itself decides.

use std::cmp::Ordering;
use std::io::{self, Read};

use yaoplm_device::fs::{find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, RelPath};
use yaoplm_formats::ps2_elf;

use super::install::{self, backup_name};
use super::manifest::{self, Manifest};
use super::{BuildVersion, Catalog};
use crate::detect::{self, Detection, Family};

/// A file this big is not read for its version.
const MAX_READ: u64 = 64 << 20;

/// How YAOPLM knows which build a loader is, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownBy {
    /// The manifest, and its SHA-256 is the file's.
    Manifest,
    /// The SHA-256 of a frozen upstream release.
    Checksum,
    /// The version string inside the ELF.
    Version,
    /// Only its name or its app's `title.cfg`: the family, maybe not the
    /// build.
    Name,
}

/// The newest backup of a loader's path: what "Roll back" puts back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backup {
    /// `.yaoplm/backup/loader/20261025-134640/UPDT_OPL.ELF`.
    pub file: String,
    pub len: u64,
    /// What the backed-up program is, if it says.
    pub family: Option<Family>,
    pub version: Option<String>,
}

/// One loader on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub path: String,
    pub family: Family,
    pub version: Option<String>,
    pub sha256: String,
    /// The catalogue build: `opl-beta`.
    pub build: Option<String>,
    pub known_by: KnownBy,
    /// The manifest names another file at this path: it was replaced
    /// without YAOPLM, and the manifest is out of date.
    pub manifest_stale: bool,
    /// The manifest's entry, while it holds.
    pub entry: Option<manifest::Installed>,
    pub backup: Option<Backup>,
}

/// How an installed build stands against the newest one of its channel
/// that YAOPLM knows of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Nothing newer is known.
    Current,
    /// The channel has a newer build: this version.
    Update(String),
    /// No release of the channel was read yet, or the versions do not
    /// order (the same build number with another commit, ADR-0042).
    Unknown,
}

/// `installed` against `latest`, the newest version of its channel YAOPLM
/// read (`None`: none read yet).
pub fn freshness(installed: Option<&str>, latest: Option<&str>) -> Freshness {
    let (Some(have), Some(newest)) = (
        installed.and_then(BuildVersion::parse),
        latest.and_then(BuildVersion::parse),
    ) else {
        return Freshness::Unknown;
    };
    match newest.channel_order(&have) {
        Some(Ordering::Greater) => Freshness::Update(newest.to_string()),
        Some(_) => Freshness::Current,
        None => Freshness::Unknown,
    }
}

fn rel(path: &str) -> io::Result<RelPath> {
    RelPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
}

/// The file at `path` as the directory names it, if it is a file.
fn file_at(fs: &dyn DeviceFs, path: &str) -> io::Result<Option<(RelPath, u64)>> {
    let path = rel(path)?;
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let entries = list_or_empty(fs, &parent)?;
    find_entry(&entries, path.file_name())
        .filter(|e| !e.is_dir)
        .map(|e| {
            parent
                .join(&e.name)
                .map(|real| (real, e.len))
                .map_err(|e| io::Error::other(e.to_string()))
        })
        .transpose()
}

/// A program's family, version and SHA-256, from its first `MAX_READ`
/// bytes.
pub(crate) fn read_program(
    fs: &dyn DeviceFs,
    path: &RelPath,
) -> io::Result<(Option<Family>, Option<String>, String)> {
    let mut bytes = Vec::new();
    fs.open(path)?.take(MAX_READ + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_READ {
        return Ok((None, None, yaoplm_hash::sha256_reader(fs.open(path)?)?));
    }
    let sha = yaoplm_hash::sha256_reader(&bytes[..])?;
    let program = ps2_elf::program(&bytes).ok();
    let family = program.as_ref().and_then(detect::program_family);
    let version = detect::stable_version(&sha).map(str::to_owned).or_else(|| {
        program
            .as_ref()
            .and_then(|p| ps2_elf::find_version(&p.bytes))
    });
    Ok((family, version, sha))
}

/// The newest backup of `path`, and what it is.
pub fn newest_backup(fs: &dyn DeviceFs, path: &str) -> io::Result<Option<Backup>> {
    let name = backup_name(path);
    let Some((folder, len)) = install::backups(fs, &name)?.into_iter().next() else {
        return Ok(None);
    };
    let file = format!("{folder}/{name}");
    let (family, version, _) = match file_at(fs, &file)? {
        Some((real, _)) => read_program(fs, &real)?,
        None => (None, None, String::new()),
    };
    Ok(Some(Backup {
        file,
        len,
        family,
        version,
    }))
}

/// The catalogue build a version of `family` belongs to.
fn build_of(catalog: &Catalog, family: Family, version: Option<&str>) -> Option<String> {
    let version = BuildVersion::parse(version?)?;
    catalog
        .channel_of_version(family, &version)
        .map(|b| b.id.clone())
}

/// Every loader on the device, in the detection's order, then the
/// manifest's own.
pub fn installed(
    fs: &dyn DeviceFs,
    detection: &Detection,
    catalog: &Catalog,
) -> io::Result<Vec<Installed>> {
    let manifest = Manifest::read(fs).map(|(m, _)| m).unwrap_or_default();
    let mut found: Vec<Installed> = Vec::new();
    for loader in &detection.loaders {
        if found
            .iter()
            .any(|f| f.path.eq_ignore_ascii_case(&loader.path))
        {
            continue;
        }
        let entry = manifest.entry(&loader.path);
        let holds = entry.filter(|e| e.sha256.eq_ignore_ascii_case(&loader.sha256));
        let by_version = build_of(catalog, loader.family, loader.version.as_deref());
        let (build, known_by) = match holds {
            Some(e) if catalog.build(&e.build).is_some() => {
                (Some(e.build.clone()), KnownBy::Manifest)
            }
            _ if loader.exact => (by_version, KnownBy::Checksum),
            _ if loader.version_in_elf => (by_version, KnownBy::Version),
            _ => (None, KnownBy::Name),
        };
        found.push(Installed {
            path: loader.path.clone(),
            family: loader.family,
            version: loader
                .version
                .clone()
                .or_else(|| holds.and_then(|e| e.version.clone())),
            sha256: loader.sha256.clone(),
            build,
            known_by,
            manifest_stale: entry.is_some() && holds.is_none(),
            entry: holds.cloned(),
            backup: newest_backup(fs, &loader.path)?,
        });
    }
    // A path of the user's own, where the detection does not look.
    for entry in &manifest.loaders {
        if found
            .iter()
            .any(|f| f.path.eq_ignore_ascii_case(&entry.path))
        {
            continue;
        }
        let Some(build) = catalog.build(&entry.build) else {
            continue;
        };
        let Some((real, _)) = file_at(fs, &entry.path)? else {
            continue;
        };
        let (_, version, sha) = read_program(fs, &real)?;
        if !sha.eq_ignore_ascii_case(&entry.sha256) {
            continue;
        }
        found.push(Installed {
            path: entry.path.clone(),
            family: build.family,
            version: version.or_else(|| entry.version.clone()),
            sha256: sha,
            build: Some(build.id.clone()),
            known_by: KnownBy::Manifest,
            manifest_stale: false,
            entry: Some(entry.clone()),
            backup: newest_backup(fs, &entry.path)?,
        });
    }
    Ok(found)
}
