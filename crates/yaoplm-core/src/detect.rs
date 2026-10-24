//! Which loader a device is prepared for (LIB-1, SPEC §5.1.1, ADR-0023).
//!
//! Signs, strongest first (research/07 §1.9):
//! 1. An installed loader. An ELF whose SHA-256 is an upstream stable
//!    release (`data/loader-hashes/opl-stable.txt`) is exact; a loader
//!    known by its file name or its app's `title.cfg` is indirect. The ELFs
//!    are packed (ps2-packer), so no version string can be read from them.
//! 2. Settings files: RiptOPL's `settings_riptopl.cfg`, wOPL's `wopl_*.cfg`,
//!    NHDDL's `nhddl/`, and `conf_opl.cfg` with keys only OPL 1.2 writes
//!    (`include/config.h`, ps2homebrew/Open-PS2-Loader v1.1.0 against
//!    `b3a21997`) or the MMCE fork's `mmce_*` keys.
//! 3. exFAT or ZSO images: OPL 1.1.0 reads neither.
//! 4. Otherwise OPL 1.2.0 beta (ADR-0004); JPG art adds a hint, since only
//!    1.1.0 still loads it.
//!
//! Builds without a profile of their own (wOPL, the MMCE fork, NHDDL) get
//! 1.2.0 beta's rules. An override for the device or an explicit setting
//! beats all of this; the front ends decide that, never the core.

use std::io;
use std::sync::OnceLock;

use yaoplm_device::fs::{DirEntry, find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, FsKind, RelPath};
use yaoplm_formats::LoaderProfile;
use yaoplm_i18n::Phrase;

use crate::plan::Notice;

/// Known ELFs of upstream stable releases: `version sha256 date file`.
static OPL_STABLE: &str = include_str!("../../../data/loader-hashes/opl-stable.txt");

/// Keys `_saveConfig()` writes in OPL 1.2.0 betas and no 1.1.0 build knows.
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

/// Folders where loaders are installed (research/07 §2.9): the root,
/// FMCB's `UPDT/`, RiptOPL's `OPL/` fallback, and `APPS/` with its app
/// folders one level down.
const LOADER_DIRS: [&str; 3] = ["", "UPDT", "OPL"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// A release identified by its checksum.
    Exact,
    /// File names, settings files, the file system or the images.
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
    MmceFork,
    Nhddl,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Self::Opl => "OPL",
            Self::RiptOpl => "RiptOPL",
            Self::Wopl => "wOPL",
            Self::MmceFork => "OPL MMCE",
            Self::Nhddl => "NHDDL",
        }
    }

    fn profile(self) -> LoaderProfile {
        match self {
            Self::RiptOpl => LoaderProfile::RiptOpl,
            Self::Opl | Self::Wopl | Self::MmceFork | Self::Nhddl => LoaderProfile::Opl120Beta,
        }
    }
}

/// A loader found on the device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundLoader {
    pub family: Family,
    /// `v1.1.0` when the checksum says so, or a `title.cfg` `Version=`.
    pub version: Option<String>,
    /// The ELF, or the settings file or folder that gave it away.
    pub path: String,
    /// Identified by its SHA-256.
    pub exact: bool,
}

impl FoundLoader {
    fn profile(&self) -> LoaderProfile {
        match (&self.family, self.exact) {
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
    /// Several loaders, a hint from JPG art, ZSO images 1.1.0 cannot show.
    pub notices: Vec<Notice>,
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

/// The family an ELF's name belongs to (research/07 §1.9 step 4).
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

/// `key=value` lines of a small text file, keys lowercased.
fn keys_and_values(bytes: &[u8]) -> Vec<(String, String)> {
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
    /// The first settings sign: (profile, reason).
    settings: Option<(LoaderProfile, Phrase)>,
}

impl Scan<'_> {
    fn elf(&mut self, path: &str, family: Family) -> io::Result<()> {
        let Some(rel) = rel(path) else {
            return Ok(());
        };
        let mut version = None;
        let mut exact = false;
        if family == Family::Opl {
            let sha = yaoplm_hash::sha256_reader(self.fs.open(&rel)?)?;
            if let Some(known) = self.known.iter().find(|k| k.sha256 == sha) {
                version = Some(known.version.clone());
                exact = true;
            }
        }
        self.loaders.push(FoundLoader {
            family,
            version,
            path: path.to_owned(),
            exact,
        });
        Ok(())
    }

    fn settings_sign(&mut self, profile: LoaderProfile, reason: Phrase) {
        self.settings.get_or_insert((profile, reason));
    }

    /// Loader ELFs and settings files in one folder.
    fn folder(&mut self, dir: &str, entries: &[DirEntry]) -> io::Result<()> {
        for entry in entries.iter().filter(|e| !e.is_dir) {
            let path = join(dir, &entry.name);
            if let Some(family) = elf_family(&entry.name) {
                self.elf(&path, family)?;
            }
            let lower = entry.name.to_ascii_lowercase();
            if lower == "settings_riptopl.cfg" || lower == "conf_riptopl.cfg" {
                self.found_by_settings(Family::RiptOpl, &path);
            } else if lower == "conf_wopl.cfg"
                || (lower.starts_with("wopl_") && lower.ends_with(".cfg"))
            {
                self.found_by_settings(Family::Wopl, &path);
            } else if lower == "conf_opl.cfg" {
                self.conf_opl(&path)?;
            }
        }
        if let Some(nhddl) = find_entry(entries, "nhddl").filter(|e| e.is_dir) {
            self.found_by_settings(Family::Nhddl, &join(dir, &nhddl.name));
        }
        Ok(())
    }

    fn found_by_settings(&mut self, family: Family, path: &str) {
        self.settings_sign(
            family.profile(),
            Phrase::reason("profileBySettings")
                .with("loader", family.name())
                .with("file", path),
        );
    }

    fn conf_opl(&mut self, path: &str) -> io::Result<()> {
        let Some(rel) = rel(path) else {
            return Ok(());
        };
        let keys = keys_and_values(&self.fs.read(&rel)?);
        if keys.iter().any(|(k, _)| k.starts_with("mmce_")) {
            self.settings_sign(
                Family::MmceFork.profile(),
                Phrase::reason("profileByMmceKeys").with("file", path),
            );
        } else if keys.iter().any(|(k, _)| OPL12_KEYS.contains(&k.as_str())) {
            self.settings_sign(
                LoaderProfile::Opl120Beta,
                Phrase::reason("profileByOpl12Keys").with("file", path),
            );
        }
        Ok(())
    }

    /// `APPS/*.ELF` and the app folders below `APPS/`, with their
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
        let Some(rel) = rel(path) else {
            return Ok(());
        };
        let keys = keys_and_values(&self.fs.read(&rel)?);
        let get = |key: &str| keys.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let version = get("version").filter(|v| !v.is_empty());
        let riptopl = version
            .as_deref()
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("riptopen"));
        // An app whose ELF has another name: `boot=` says which.
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
        if let Some(version) = version {
            for loader in &mut self.loaders[before..] {
                if !loader.exact {
                    if riptopl {
                        loader.family = Family::RiptOpl;
                    }
                    loader.version = Some(version.clone());
                }
            }
        }
        Ok(())
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
        settings: None,
    };
    for dir in LOADER_DIRS {
        let entries = match rel(dir) {
            Some(path) => list_or_empty(fs, &path)?,
            None => continue,
        };
        scan.folder(dir, &entries)?;
    }
    scan.apps()?;
    let Scan {
        loaders, settings, ..
    } = scan;

    let images = zso_images(fs)?;
    let exfat = fs.info().is_ok_and(|info| info.kind == FsKind::ExFat);
    let mut notices = Vec::new();

    // 1. Loaders: a checksum beats names; names that disagree decide nothing.
    let exact = loaders.iter().find(|l| l.exact);
    let named = loaders.first().filter(|first| {
        loaders
            .iter()
            .all(|l| l.profile() == first.profile() && !l.exact)
    });
    let families: Vec<Family> = {
        let mut seen = Vec::new();
        for loader in &loaders {
            if !seen.contains(&loader.family) {
                seen.push(loader.family);
            }
        }
        seen
    };
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
    let (profile, confidence, reason) = if let Some(loader) = exact {
        (
            loader.profile(),
            Confidence::Exact,
            Phrase::reason("profileByChecksum")
                .with("loader", loader.label())
                .with("path", &loader.path),
        )
    } else if let Some(loader) = named {
        let reason = match &loader.version {
            Some(version) => Phrase::reason("profileByVersion")
                .with("loader", loader.family.name())
                .with("version", version)
                .with("path", &loader.path),
            None => Phrase::reason("profileByLoader")
                .with("loader", loader.family.name())
                .with("path", &loader.path),
        };
        (loader.profile(), Confidence::Indirect, reason)
    } else if let Some((profile, reason)) = settings {
        // 2. Settings files.
        (profile, Confidence::Indirect, reason)
    } else if exfat {
        // 3. What OPL 1.1.0 cannot read.
        (
            LoaderProfile::Opl120Beta,
            Confidence::Indirect,
            Phrase::reason("profileByExfat"),
        )
    } else if images > 0 {
        (
            LoaderProfile::Opl120Beta,
            Confidence::Indirect,
            Phrase::reason("profileByZso").with("images", images),
        )
    } else {
        (
            LoaderProfile::Opl120Beta,
            Confidence::Default,
            Phrase::reason("profileNoSigns"),
        )
    };

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

/// JPG pictures in `ART/`, which only OPL 1.1.0 loads.
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
    }

    #[test]
    fn settings_files_and_their_keys() {
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nusb_prefix=\r\n");
        // Keys 1.1.0 knows too say nothing about the version.
        assert_eq!(detect(&fs).confidence, Confidence::Default);

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"usb_mode=0\r\nenable_bgm=1\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::Opl120Beta, "profileByOpl12Keys")
        );

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("conf_opl.cfg", b"mmce_mode=2\r\n");
        assert_eq!(reason(&detect(&fs)), "profileByMmceKeys");

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("APPS/APP_RIPTOPL/settings_riptopl.cfg", b"enable_usb=1\r\n");
        let d = detect(&fs);
        assert_eq!(
            (d.profile, reason(&d)),
            (LoaderProfile::RiptOpl, "profileBySettings")
        );
        assert_eq!(
            d.reason.english(),
            "RiptOPL settings in APPS/APP_RIPTOPL/settings_riptopl.cfg"
        );

        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("nhddl/global.yaml", b"");
        let d = detect(&fs);
        assert_eq!(d.profile, LoaderProfile::Opl120Beta);
        assert_eq!(d.reason.english(), "NHDDL settings in nhddl");
    }

    #[test]
    fn a_stable_release_is_known_by_its_checksum() {
        let elf = b"\x7fELF not really OPL";
        let sha = yaoplm_hash::sha256_reader(&elf[..]).unwrap();
        let table = format!("# test\nv1.1.0 {sha} 2021-09-09 OPNPS2LD-v1.1.0.ELF\n");
        let fs = MemFs::new(FsKind::Fat32);
        fs.insert_file("UPDT/OPL.ELF", elf);
        fs.insert_file("DVD/Game.zso", b"x");
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
        // The strongest sign wins and the conflict is said.
        assert_eq!(codes(&d), ["zsoHiddenFromOpl110"]);

        // Any other OPL build: the family, with 1.2.0 beta's rules.
        let d = detect(&fs);
        assert_eq!(
            (d.profile, d.confidence, reason(&d)),
            (
                LoaderProfile::Opl120Beta,
                Confidence::Indirect,
                "profileByLoader"
            )
        );
        assert!(d.notices.is_empty());
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
    }
}
