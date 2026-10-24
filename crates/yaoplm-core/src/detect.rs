//! Which loader a device is prepared for (LIB-1, SPEC §5.1.1, ADR-0023).
//!
//! Signs by strength (research/16 §9–10), the first level that agrees
//! decides:
//! 1. An ELF whose SHA-256 is an upstream stable release
//!    (`data/loader-hashes/opl-stable.txt`): exact. ELFs are packed by
//!    ps2-packer, so their version string cannot be read.
//! 2. Files only one family writes: RiptOPL's `title.cfg` `Version=` and its
//!    settings and state files, wOPL's and uOPL's settings, NHDDL's caches.
//! 3. `CD/games.bin`, `DVD/games.bin`: 92-byte records were last written by
//!    OPL 1.1.0 or a beta up to build 2016, 188-byte ones by a later beta or
//!    a fork (build numbers, not years).
//! 4. `conf_opl.cfg`: OPL saves every key it knows, so the keys every beta
//!    from 1871 on writes (`bdm_cache`, `enable_bgm`, `x_sensitivity`,
//!    `enable_bdm_hdd`) prove a 1.2 build saved it, and their absence that
//!    none did (`include/config.h`, `src/opl.c` `_saveConfig()`).
//! 5. ELF names: `RIPTOPL*`, `WOPNPS2LD*`, `nhddl*`, `OPNPS2LD*`, `OPL.ELF`.
//!
//! exFAT and ZSO images rule out OPL 1.1.0 (and betas before 1880/1871) for
//! levels 3 to 5; with nothing else they mean OPL 1.2.0 beta. A stronger
//! sign for 1.1.0 on such a device stays, and the device's problems say
//! why it will not work. Without signs: OPL 1.2.0 beta (ADR-0004); JPG art
//! then adds a hint. Builds without a profile of their own (wOPL, uOPL,
//! NHDDL, the MMCE fork, YAOPL) get 1.2.0 beta's rules. An override for
//! the device or an explicit setting beats all of this; the front ends
//! decide that, never the core.

use std::io;
use std::sync::OnceLock;

use yaoplm_device::fs::{DirEntry, find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, FsKind, RelPath};
use yaoplm_formats::LoaderProfile;
use yaoplm_formats::games_bin::{self, Generation};
use yaoplm_i18n::Phrase;

use crate::plan::Notice;

/// Known ELFs of upstream stable releases: `version sha256 date file`.
static OPL_STABLE: &str = include_str!("../../../data/loader-hashes/opl-stable.txt");

/// Keys `_saveConfig()` writes on every save from these betas on: 1871
/// (`bdm_cache`), 1954 (`enable_bgm`), 2129 (`x_sensitivity`), 2194
/// (`enable_bdm_hdd`). Forks of 1.2 write them too; `mmce_*` comes from
/// the MMCE fork and YAOPL (research/16 §1.2, §2, §3).
const OPL12_KEYS: [&str; 9] = [
    "bdm_cache",
    "bgm_volume",
    "default_bgm_path",
    "enable_bdm_hdd",
    "enable_bgm",
    "hdd_cache",
    "smb_cache",
    "x_sensitivity",
    "y_sensitivity",
];

/// Keys every OPL since 1.0.0 saves: a `conf_opl.cfg` with one of them is
/// OPL's own.
const OPL_KEYS: [&str; 3] = ["usb_mode", "default_device", "enable_delete_rename"];

/// Folders where loaders are installed (research/07 §2.9): the root,
/// FMCB's `UPDT/`, RiptOPL's `OPL/` fallback; `APPS/` and its app folders
/// are read on their own.
const LOADER_DIRS: [&str; 3] = ["", "UPDT", "OPL"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A release identified by its checksum.
    Exact,
    /// Files on the device, the file system or the images.
    Indirect,
    /// No signs: the default profile.
    Default,
}

/// A loader family YAOPLM recognises on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Opl,
    RiptOpl,
    Wopl,
    Uopl,
    Nhddl,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Self::Opl => "OPL",
            Self::RiptOpl => "RiptOPL",
            Self::Wopl => "wOPL",
            Self::Uopl => "uOPL",
            Self::Nhddl => "NHDDL",
        }
    }

    fn profile(self) -> LoaderProfile {
        match self {
            Self::RiptOpl => LoaderProfile::RiptOpl,
            Self::Opl | Self::Wopl | Self::Uopl | Self::Nhddl => LoaderProfile::Opl120Beta,
        }
    }
}

/// A loader found on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundLoader {
    pub family: Family,
    /// `v1.1.0` when the checksum says so, or a `title.cfg` `Version=`.
    pub version: Option<String>,
    /// The ELF.
    pub path: String,
    /// Identified by its SHA-256.
    pub exact: bool,
}

impl FoundLoader {
    fn profile(&self) -> LoaderProfile {
        match (self.family, self.exact) {
            // Only upstream's stable releases are in the table: 1.0.0 and
            // 1.1.0 share 1.1.0's rules.
            (Family::Opl, true) => LoaderProfile::Opl110,
            (family, _) => family.profile(),
        }
    }

    fn label(&self) -> String {
        match &self.version {
            Some(version) if self.exact => format!("{} {version}", self.family.name()),
            _ => self.family.name().to_owned(),
        }
    }
}

/// What the device says about its loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub profile: LoaderProfile,
    pub confidence: Confidence,
    /// Why, to follow a colon after the profile's name (`reasons.*`).
    pub reason: Phrase,
    pub loaders: Vec<FoundLoader>,
    /// Several loaders, `games.bin` from another generation, ZSO for
    /// 1.1.0, a hint from JPG art.
    pub notices: Vec<Notice>,
}

/// Strength of a sign, strongest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Checksum,
    FamilyFiles,
    GamesBin,
    ConfOpl,
    ElfName,
}

#[derive(Debug, Clone)]
struct Sign {
    level: Level,
    profile: LoaderProfile,
    reason: Phrase,
}

struct KnownElf {
    version: String,
    sha256: String,
}

fn known_elfs() -> &'static [KnownElf] {
    static KNOWN: OnceLock<Vec<KnownElf>> = OnceLock::new();
    KNOWN.get_or_init(|| parse_known(OPL_STABLE))
}

fn parse_known(text: &str) -> Vec<KnownElf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            Some(KnownElf {
                version: fields.next()?.to_owned(),
                sha256: fields.next()?.to_ascii_lowercase(),
            })
        })
        .collect()
}

fn rel(path: &str) -> Option<RelPath> {
    RelPath::new(path).ok()
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// The family an ELF's name belongs to (research/16 §9, sign 9).
fn elf_family(name: &str) -> Option<Family> {
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".elf") {
        return None;
    }
    if lower.starts_with("riptopl") {
        Some(Family::RiptOpl)
    } else if lower.starts_with("wopnps2ld") {
        Some(Family::Wopl)
    } else if lower.starts_with("nhddl") {
        Some(Family::Nhddl)
    } else if lower.starts_with("opnps2ld") || lower == "opl.elf" {
        Some(Family::Opl)
    } else {
        None
    }
}

/// A settings or state file only one family writes, by name.
fn family_file(name: &str) -> Option<Family> {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "settings_riptopl.cfg" | "conf_riptopl.cfg" | "config.path" => Some(Family::RiptOpl),
        "conf_wopl.cfg" | "conf_wopl.cfg.bak" | "wopl_boot.cfg" => Some(Family::Wopl),
        "conf_uopl.cfg" => Some(Family::Uopl),
        _ if lower.starts_with("wopl_") && lower.ends_with(".cfg") => Some(Family::Wopl),
        _ => None,
    }
}

/// `key=value` lines of a small text file, keys lowercased.
fn keys(bytes: &[u8]) -> Vec<(String, String)> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect()
}

struct Scan<'a> {
    fs: &'a dyn DeviceFs,
    known: &'a [KnownElf],
    loaders: Vec<FoundLoader>,
    signs: Vec<Sign>,
}

impl Scan<'_> {
    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        match rel(path) {
            Some(rel) => self.fs.read(&rel),
            None => Ok(Vec::new()),
        }
    }

    fn starts_with(&self, path: &str, magic: &[u8]) -> io::Result<bool> {
        Ok(self.read(path)?.starts_with(magic))
    }

    fn sign(&mut self, level: Level, profile: LoaderProfile, reason: Phrase) {
        self.signs.push(Sign {
            level,
            profile,
            reason,
        });
    }

    fn by_file(&mut self, family: Family, path: &str) {
        self.sign(
            Level::FamilyFiles,
            family.profile(),
            Phrase::reason("profileByFiles")
                .with("loader", family.name())
                .with("file", path),
        );
    }

    fn elf(&mut self, path: &str, family: Family) -> io::Result<()> {
        let Some(rel) = rel(path) else {
            return Ok(());
        };
        let mut found = FoundLoader {
            family,
            version: None,
            path: path.to_owned(),
            exact: false,
        };
        if family == Family::Opl {
            let sha = yaoplm_hash::sha256_reader(self.fs.open(&rel)?)?;
            if let Some(known) = self.known.iter().find(|k| k.sha256 == sha) {
                found.version = Some(known.version.clone());
                found.exact = true;
            }
        }
        let (level, reason) = if found.exact {
            (
                Level::Checksum,
                Phrase::reason("profileByChecksum")
                    .with("loader", found.label())
                    .with("path", path),
            )
        } else {
            (
                Level::ElfName,
                Phrase::reason("profileByLoader")
                    .with("loader", family.name())
                    .with("path", path),
            )
        };
        self.sign(level, found.profile(), reason);
        self.loaders.push(found);
        Ok(())
    }

    /// Loader ELFs and family files in one folder.
    fn folder(&mut self, dir: &str, entries: &[DirEntry]) -> io::Result<()> {
        for entry in entries.iter().filter(|e| !e.is_dir) {
            let path = join(dir, &entry.name);
            if let Some(family) = elf_family(&entry.name) {
                self.elf(&path, family)?;
            } else if let Some(family) = family_file(&entry.name) {
                self.by_file(family, &path);
            } else if entry.name.eq_ignore_ascii_case("favourites.bin")
                && self.starts_with(&path, b"OFAV")?
            {
                self.by_file(Family::RiptOpl, &path);
            } else if entry.name.eq_ignore_ascii_case("conf_opl.cfg") {
                self.conf_opl(&path)?;
            }
        }
        Ok(())
    }

    fn conf_opl(&mut self, path: &str) -> io::Result<()> {
        let keys = keys(&self.read(path)?);
        let has = |names: &[&str]| keys.iter().any(|(k, _)| names.contains(&k.as_str()));
        if has(&OPL12_KEYS) || keys.iter().any(|(k, _)| k.starts_with("mmce_")) {
            self.sign(
                Level::ConfOpl,
                LoaderProfile::Opl120Beta,
                Phrase::reason("profileByOpl12Keys").with("file", path),
            );
        } else if has(&OPL_KEYS) {
            self.sign(
                Level::ConfOpl,
                LoaderProfile::Opl110,
                Phrase::reason("profileByOpl110Keys").with("file", path),
            );
        }
        Ok(())
    }

    /// `APPS/*.ELF` and the app folders below `APPS/` with their
    /// `title.cfg` (`boot=`, RiptOPL's `Version=`).
    fn apps(&mut self) -> io::Result<()> {
        let Some(apps) = rel("APPS") else {
            return Ok(());
        };
        let entries = list_or_empty(self.fs, &apps)?;
        self.folder("APPS", &entries)?;
        for app in entries.iter().filter(|e| e.is_dir) {
            let dir = join("APPS", &app.name);
            let Some(app_rel) = rel(&dir) else {
                continue;
            };
            let inside = list_or_empty(self.fs, &app_rel)?;
            let before = self.loaders.len();
            self.folder(&dir, &inside)?;
            if let Some(title) = find_entry(&inside, "title.cfg").filter(|e| !e.is_dir) {
                self.title_cfg(&dir, &inside, &join(&dir, &title.name), before)?;
            }
        }
        Ok(())
    }

    fn title_cfg(
        &mut self,
        dir: &str,
        inside: &[DirEntry],
        path: &str,
        before: usize,
    ) -> io::Result<()> {
        let keys = keys(&self.read(path)?);
        let get = |key: &str| keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let version = get("version").filter(|v| !v.is_empty());
        let riptopl = version
            .as_deref()
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("riptopen"));
        // An app whose ELF has its own name: `boot=` says which.
        if self.loaders.len() == before
            && let Some(boot) = get("boot")
            && let Some(elf) = find_entry(inside, &boot).filter(|e| !e.is_dir)
        {
            let title = get("title").unwrap_or_default().to_ascii_lowercase();
            let family = if riptopl {
                Some(Family::RiptOpl)
            } else if title.contains("open ps2 loader") || title == "opl" {
                Some(Family::Opl)
            } else {
                None
            };
            if let Some(family) = family {
                self.elf(&join(dir, &elf.name), family)?;
            }
        }
        if !riptopl {
            return Ok(());
        }
        let version = version.unwrap_or_default();
        for loader in self.loaders[before..].iter_mut().filter(|l| !l.exact) {
            loader.family = Family::RiptOpl;
            loader.version = Some(version.clone());
            self.signs.push(Sign {
                level: Level::FamilyFiles,
                profile: LoaderProfile::RiptOpl,
                reason: Phrase::reason("profileByVersion")
                    .with("loader", Family::RiptOpl.name())
                    .with("version", &version)
                    .with("path", &loader.path),
            });
        }
        Ok(())
    }

    /// NHDDL's caches: `nhddl/` alone is also made by the PSBBN installer.
    fn nhddl(&mut self) -> io::Result<()> {
        let Some(dir) = rel("nhddl") else {
            return Ok(());
        };
        let entries = list_or_empty(self.fs, &dir)?;
        if let Some(cache) = find_entry(&entries, "cache.bin").filter(|e| !e.is_dir) {
            let path = join("nhddl", &cache.name);
            if self.starts_with(&path, b"NIDC")? {
                self.by_file(Family::Nhddl, &path);
            }
        }
        if let Some(last) = find_entry(&entries, "lastTitle.bin").filter(|e| !e.is_dir) {
            self.by_file(Family::Nhddl, &join("nhddl", &last.name));
        }
        Ok(())
    }

    /// RiptOPL's art cache: `ART/art_cache.bin` starting `ARC\0`.
    fn art_cache(&mut self) -> io::Result<()> {
        let Some(dir) = rel("ART") else {
            return Ok(());
        };
        if let Some(cache) = find_entry(&list_or_empty(self.fs, &dir)?, "art_cache.bin") {
            let path = join("ART", &cache.name);
            if !cache.is_dir && self.starts_with(&path, b"ARC\0")? {
                self.by_file(Family::RiptOpl, &path);
            }
        }
        Ok(())
    }

    /// The generation of OPL that last listed `CD/` and `DVD/`.
    fn games_bin(&mut self) -> io::Result<Option<Generation>> {
        let mut found = None;
        for folder in ["CD", "DVD"] {
            let Some(dir) = rel(folder) else { continue };
            let entries = list_or_empty(self.fs, &dir)?;
            let Some(entry) = find_entry(&entries, games_bin::FILE_NAME).filter(|e| !e.is_dir)
            else {
                continue;
            };
            let path = join(folder, &entry.name);
            let Some(generation) = games_bin::generation(&self.read(&path)?) else {
                continue;
            };
            let (profile, key) = match generation {
                Generation::Old => (LoaderProfile::Opl110, "profileByOldGamesBin"),
                Generation::New => (LoaderProfile::Opl120Beta, "profileByNewGamesBin"),
            };
            self.sign(
                Level::GamesBin,
                profile,
                Phrase::reason(key).with("file", &path),
            );
            found = match found {
                None => Some(generation),
                Some(other) if other == generation => Some(generation),
                // Two generations: the newer listed one folder last.
                Some(_) => Some(Generation::New),
            };
        }
        Ok(found)
    }
}

/// Reads the device's signs. Errors are those of reading the device.
pub fn detect_profile(fs: &dyn DeviceFs) -> io::Result<Detection> {
    detect_with(fs, known_elfs())
}

fn detect_with(fs: &dyn DeviceFs, known: &[KnownElf]) -> io::Result<Detection> {
    let mut scan = Scan {
        fs,
        known,
        loaders: Vec::new(),
        signs: Vec::new(),
    };
    for dir in LOADER_DIRS {
        let entries = match rel(dir) {
            Some(path) => list_or_empty(fs, &path)?,
            None => continue,
        };
        scan.folder(dir, &entries)?;
    }
    scan.apps()?;
    scan.nhddl()?;
    scan.art_cache()?;
    let listed_by = scan.games_bin()?;
    let Scan { loaders, signs, .. } = scan;

    let images = zso_images(fs)?;
    let exfat = fs.info().is_ok_and(|info| info.kind == FsKind::ExFat);
    // exFAT and ZSO beat what weaker signs say about 1.1.0.
    let not_110 = exfat || images > 0;
    let mut notices = Vec::new();

    let mut chosen: Option<(Level, &Sign)> = None;
    for level in [
        Level::Checksum,
        Level::FamilyFiles,
        Level::GamesBin,
        Level::ConfOpl,
        Level::ElfName,
    ] {
        let here: Vec<&Sign> = signs
            .iter()
            .filter(|s| s.level == level)
            .filter(|s| {
                !(not_110 && level > Level::FamilyFiles && s.profile == LoaderProfile::Opl110)
            })
            .collect();
        let Some(first) = here.first() else { continue };
        // Signs of one level that disagree decide nothing.
        if here.iter().all(|s| s.profile == first.profile) {
            chosen = Some((level, first));
            break;
        }
    }

    let (profile, confidence, reason) = match chosen {
        Some((level, sign)) => (
            sign.profile,
            if level == Level::Checksum {
                Confidence::Exact
            } else {
                Confidence::Indirect
            },
            sign.reason.clone(),
        ),
        None if exfat => (
            LoaderProfile::Opl120Beta,
            Confidence::Indirect,
            Phrase::reason("profileByExfat"),
        ),
        None if images > 0 => (
            LoaderProfile::Opl120Beta,
            Confidence::Indirect,
            Phrase::reason("profileByZso").with("images", images),
        ),
        None => (
            LoaderProfile::Opl120Beta,
            Confidence::Default,
            Phrase::reason("profileNoSigns"),
        ),
    };

    let families = loaders.iter().fold(Vec::new(), |mut seen, l| {
        if !seen.contains(&l.family) {
            seen.push(l.family);
        }
        seen
    });
    if loaders.len() > 1 && (families.len() > 1 || loaders.iter().any(|l| l.exact)) {
        let list = loaders
            .iter()
            .map(|l| format!("{} ({})", l.label(), l.path))
            .collect::<Vec<_>>()
            .join(", ");
        notices.push(Notice::info(
            Phrase::notice("severalLoaders").with("loaders", list),
        ));
    }
    // An installed loader against the generation that last listed the
    // games: the console may start another copy (research/16 §10).
    let chosen_level = chosen.map(|(level, _)| level);
    if let Some(generation) = listed_by
        && chosen_level.is_some_and(|level| level != Level::GamesBin)
    {
        let listed = match generation {
            Generation::Old => LoaderProfile::Opl110,
            Generation::New => LoaderProfile::Opl120Beta,
        };
        let rules = |p: LoaderProfile| p == LoaderProfile::Opl110;
        if rules(listed) != rules(profile) {
            let code = match generation {
                Generation::Old => "listedByOldOpl",
                Generation::New => "listedByNewOpl",
            };
            notices.push(Notice::info(Phrase::notice(code)));
        }
    }
    if profile == LoaderProfile::Opl110 && images > 0 {
        notices.push(Notice::warning(
            Phrase::notice("zsoHiddenFromOpl110").with("images", images),
        ));
    }
    if confidence == Confidence::Default {
        let jpg = jpg_art(fs)?;
        if jpg > 0 {
            notices.push(Notice::info(
                Phrase::notice("jpgArtHint").with("files", jpg),
            ));
        }
    }
    Ok(Detection {
        profile,
        confidence,
        reason,
        loaders,
        notices,
    })
}

/// ZSO images in `CD/` and `DVD/`.
fn zso_images(fs: &dyn DeviceFs) -> io::Result<u64> {
    let mut count = 0;
    for folder in ["CD", "DVD"] {
        let Some(dir) = rel(folder) else { continue };
        count += list_or_empty(fs, &dir)?
            .iter()
            .filter(|e| !e.is_dir && e.name.to_ascii_lowercase().ends_with(".zso"))
            .count() as u64;
    }
    Ok(count)
}

/// JPG pictures in `ART/`, which betas from build 2169 on no longer load.
fn jpg_art(fs: &dyn DeviceFs) -> io::Result<u64> {
    let Some(dir) = rel("ART") else { return Ok(0) };
    Ok(list_or_empty(fs, &dir)?
        .iter()
        .filter(|e| {
            let lower = e.name.to_ascii_lowercase();
            !e.is_dir && (lower.ends_with(".jpg") || lower.ends_with(".jpeg"))
        })
        .count() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yaoplm_test_support::MemFs;

    fn detect(fs: &MemFs) -> Detection {
        detect_profile(fs).unwrap()
    }

    fn reason(d: &Detection) -> &str {
        d.reason.key.trim_start_matches("reasons.")
    }

    fn codes(d: &Detection) -> Vec<&str> {
        d.notices.iter().map(|n| n.code.as_str()).collect()
    }

    /// A `games.bin` of `size`-byte records naming one ISO.
    fn games_bin(size: usize) -> Vec<u8> {
        let mut record = vec![0u8; size];
        let ext = if size == 92 { 78 } else { 174 };
        record[..4].copy_from_slice(b"Game");
        record[ext..ext + 4].copy_from_slice(b".iso");
        record
    }

    #[test]
    fn the_bundled_table_has_both_stable_releases() {
        let known = known_elfs();
        assert_eq!(known.iter().filter(|k| k.version == "v1.1.0").count(), 17);
        assert_eq!(known.iter().filter(|k| k.version == "v1.0.0").count(), 17);
        assert!(known.iter().all(|k| k.sha256.len() == 64));
        // The release's main ELF.
        assert!(known.iter().any(
            |k| k.sha256 == "dd6036a06a5c3ec6482625f2e4687af76448fe46a1b58049c00ea35741499520"
        ));
    }

    #[test]
    fn no_signs_is_the_default_with_a_hint_from_jpg_art() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("CD/SLUS_200.62.Game.iso", b"x");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, d.confidence, reason(&d)),
            (
                LoaderProfile::Opl120Beta,
                Confidence::Default,
                "profileNoSigns"
            )
        );
        assert!(d.notices.is_empty());

        fs.insert_file("ART/SLUS_200.62_COV.jpg", b"x");
        assert_eq!(codes(&detect(&fs)), ["jpgArtHint"]);
        // `nhddl/` alone is made by the PSBBN installer too.
        fs.insert_file("nhddl/global.yaml", b"");
        assert_eq!(detect(&fs).confidence, Confidence::Default);
    }

    #[test]
    fn exfat_and_zso_rule_out_opl_110() {
        let d = detect(&MemFs::new(FsKind::ExFat));
        assert_eq!(
            (d.profile, d.confidence, reason(&d)),
            (
                LoaderProfile::Opl120Beta,
                Confidence::Indirect,
                "profileByExfat"
            )
        );
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("DVD/Game.zso", b"x");
        fs.insert_file("ART/X_COV.jpg", b"x");
        let d = detect(&fs);
        assert_eq!(reason(&d), "profileByZso");
        // No hint: the profile is not a default.
        assert!(d.notices.is_empty());

        // An old games.bin or a 1.1.0 conf_opl.cfg cannot beat them.
        fs.insert_file("CD/games.bin", &games_bin(92));
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl120Beta, "profileByZso")
        );
    }

    #[test]
    fn conf_opl_keys() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nenable_ilink=0\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl110, "profileByOpl110Keys")
        );

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nenable_bgm=1\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl120Beta, "profileByOpl12Keys")
        );

        // YAOPL and the MMCE fork: a 1.2 build either way.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nmmce_mode=2\r\n");
        assert_eq!(reason(&detect(&fs)), "profileByOpl12Keys");

        // Not OPL's: no sign.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"hello=1\r\n");
        assert_eq!(detect(&fs).confidence, Confidence::Default);
    }

    #[test]
    fn games_bin_tells_the_generation_that_listed_last() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("DVD/games.bin", &games_bin(92));
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl110, "profileByOldGamesBin")
        );
        assert_eq!(
            d.reason.english(),
            "DVD/games.bin was last written by OPL 1.1.0 or a beta up to build 2016"
        );
        // It beats the settings file's keys.
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nenable_bgm=1\r\n");
        assert_eq!(detect(&fs).profile, LoaderProfile::Opl110);

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("CD/games.bin", &games_bin(188));
        assert_eq!(reason(&detect(&fs)), "profileByNewGamesBin");
    }

    #[test]
    fn family_files() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("APPS/APP_RIPTOPL/settings_riptopl.cfg", b"enable_usb=1\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::RiptOpl, "profileByFiles")
        );
        assert_eq!(
            d.reason.english(),
            "APPS/APP_RIPTOPL/settings_riptopl.cfg, which only RiptOPL writes"
        );

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("favourites.bin", b"OFAV\x01");
        assert_eq!(detect(&fs).profile, LoaderProfile::RiptOpl);
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("favourites.bin", b"something else");
        assert_eq!(detect(&fs).confidence, Confidence::Default);

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("ART/art_cache.bin", b"ARC\0\x04");
        assert_eq!(detect(&fs).profile, LoaderProfile::RiptOpl);

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("nhddl/cache.bin", b"NIDC....");
        let d = detect(&fs);
        assert_eq!(
            d.reason.english(),
            "nhddl/cache.bin, which only NHDDL writes"
        );
        assert_eq!(d.profile, LoaderProfile::Opl120Beta);

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("wopl_settings.cfg", b"");
        assert_eq!(
            detect(&fs).reason.english(),
            "wopl_settings.cfg, which only wOPL writes"
        );
    }

    #[test]
    fn a_stable_release_is_known_by_its_checksum() {
        let elf = b"\x7fELF not really OPL";
        let sha = yaoplm_hash::sha256_reader(&elf[..]).unwrap();
        let table = format!("# test\nv1.1.0 {sha} 2021-09-09 OPNPS2LD-v1.1.0.ELF\n");
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("UPDT/OPL.ELF", elf);
        fs.insert_file("DVD/Game.zso", b"x");
        fs.insert_file("DVD/games.bin", &games_bin(188));
        let d = detect_with(&fs, &parse_known(&table)).unwrap();
        assert_eq!(
            (d.profile, d.confidence, reason(&d)),
            (
                LoaderProfile::Opl110,
                Confidence::Exact,
                "profileByChecksum"
            )
        );
        assert_eq!(
            d.reason.english(),
            "OPL v1.1.0 at UPDT/OPL.ELF, known by its checksum"
        );
        // The strongest sign wins and the conflicts are said.
        assert_eq!(codes(&d), ["listedByNewOpl", "zsoHiddenFromOpl110"]);

        // Any other OPL build: the generation of games.bin decides.
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl120Beta, "profileByNewGamesBin")
        );
    }

    #[test]
    fn app_folders_and_their_title_cfg() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("APPS/APP_RIPTOPL/RIPTOPL.ELF", b"elf");
        fs.insert_file(
            "APPS/APP_RIPTOPL/title.cfg",
            b"title=RiptOPL\nboot=RIPTOPL.ELF\nVersion=RiptOpen PS2 Loader v1.2.0 OFFICIAL ROLLING\n",
        );
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::RiptOpl, "profileByVersion")
        );
        assert_eq!(
            d.loaders[0].version.as_deref(),
            Some("RiptOpen PS2 Loader v1.2.0 OFFICIAL ROLLING")
        );

        // An app whose ELF has its own name, found through `boot=`.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("APPS/MYOPL/MINE.ELF", b"elf");
        fs.insert_file(
            "APPS/MYOPL/title.cfg",
            b"title=Open PS2 Loader\nboot=MINE.ELF\n",
        );
        let d = detect(&fs);
        assert_eq!(d.loaders.len(), 1);
        assert_eq!(d.loaders[0].path, "APPS/MYOPL/MINE.ELF");
        assert_eq!(d.loaders[0].family, Family::Opl);
        assert_eq!(reason(&d), "profileByLoader");
    }

    #[test]
    fn loaders_that_disagree_decide_nothing() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("APPS/OPNPS2LD.ELF", b"one");
        fs.insert_file("APPS/APP_RIPTOPL/RIPTOPL.ELF", b"two");
        let d = detect(&fs);
        assert_eq!(d.loaders.len(), 2);
        assert_eq!(
            (d.profile, d.confidence),
            (LoaderProfile::Opl120Beta, Confidence::Default)
        );
        assert_eq!(codes(&d), ["severalLoaders"]);
        assert!(
            d.notices[0]
                .message
                .contains("RiptOPL (APPS/APP_RIPTOPL/RIPTOPL.ELF)")
        );

        // A settings file then breaks the tie.
        fs.insert_file("APPS/APP_RIPTOPL/settings_riptopl.cfg", b"");
        assert_eq!(detect(&fs).profile, LoaderProfile::RiptOpl);

        // Two copies of one family agree.
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("UPDT/OPL.ELF", b"a");
        fs.insert_file("APPS/OPNPS2LD.ELF", b"b");
        let d = detect(&fs);
        assert_eq!(reason(&d), "profileByLoader");
        assert!(d.notices.is_empty());
    }

    #[test]
    fn names() {
        assert_eq!(elf_family("OPNPS2LD.ELF"), Some(Family::Opl));
        assert_eq!(elf_family("opnps2ld-v1.2.0-Beta.elf"), Some(Family::Opl));
        assert_eq!(elf_family("OPL.ELF"), Some(Family::Opl));
        assert_eq!(elf_family("RIPTOPL.ELF"), Some(Family::RiptOpl));
        assert_eq!(elf_family("WOPNPS2LD.ELF"), Some(Family::Wopl));
        assert_eq!(elf_family("nhddl-ata.elf"), Some(Family::Nhddl));
        assert_eq!(elf_family("BOOT.ELF"), None);
        assert_eq!(elf_family("OPNPS2LD.7z"), None);
        assert_eq!(family_file("CONF_UOPL.CFG"), Some(Family::Uopl));
        assert_eq!(family_file("wopl_boot.cfg"), Some(Family::Wopl));
        assert_eq!(family_file("conf_opl.cfg"), None);
    }
}
