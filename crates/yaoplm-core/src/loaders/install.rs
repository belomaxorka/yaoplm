//! Installing a loader build on a device: one plan (SPEC LDR-3, §5.10.1,
//! ADR-0042, ADR-0044). In order:
//! 1. the folders the build reads (LIB-3);
//! 2. per path: the ELF into `<path>.yaoplm-part`, synced and renamed into
//!    place, the file it replaces first renamed into
//!    `.yaoplm/backup/loader/<time>/` (three backups a path are kept);
//!    an app folder also gets its `title.cfg`;
//! 3. the language pack, flat in `LNG/`;
//! 4. the manifest, `.yaoplm/manifest.json`;
//! 5. starter settings in `conf_opl.cfg`, only when asked for and only for a
//!    USB device, other keys and their order kept.
//!
//! An update (LDR-4) is the same plan for the one path the build is at:
//! no folders, no `title.cfg`, no settings, only the ELF, its language
//! pack and the manifest.
//!
//! What sits at a path decides how: the same ELF is left alone; another OPL
//! build is replaced; a file of another family, or one YAOPLM does not
//! recognise, only with the user's "replace this file" (the backup is made
//! all the same). Nothing is deleted. The memory card, launcher configs and
//! the service files of a memory card emulator's SD are never touched.

use std::io::{self, Read};
use std::path::PathBuf;

use yaoplm_device::fs::{find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, RelPath};
use yaoplm_formats::cfg::CfgDocument;
use yaoplm_formats::ps2_elf;
use yaoplm_i18n::Phrase;

use super::catalog::{Build, Target};
use super::languages::LanguageFile;
use super::manifest::{self, Installed, Manifest};
use crate::detect::{self, Family};
use crate::device::DeviceState;
use crate::ops::new_plan;
use crate::plan::{
    Bytes, Expect, Level, Notice, Plan, PlanKind, Removal, Source, SourceFile, SourceKind, Step,
    Transaction,
};

/// Backups kept per path, the new one included (ADR-0042).
pub const KEEP_BACKUPS: usize = 3;
pub const BACKUP_DIR: &str = ".yaoplm/backup/loader";
/// Top-level names a memory card emulator keeps on its SD (SD2PSX and
/// sd2psXtd, MemCard PRO 2; research/07 §2.7): never written to.
const EMULATOR_FILES: [&str; 6] = [".sd2psx", "MemoryCards", "PS1", "PS2", "mcp2.bin", "os"];
/// A file this big at a loader's path is not read to tell what it is.
const MAX_READ: u64 = 64 << 20;

/// The ELF to install: a file of the app's loader store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElfFile {
    pub path: PathBuf,
    pub sha256: String,
    pub len: u64,
    pub modified: Option<u128>,
    pub version: Option<String>,
    /// The release tag; `None` for the user's own file.
    pub tag: Option<String>,
    /// The release page or the user's file, for the manifest.
    pub source: String,
}

/// One place to put the ELF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub path: String,
    /// The catalogue preset, `None` for a path the user typed.
    pub preset: Option<String>,
    /// An app folder's `title=`.
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallRequest<'a> {
    pub build: &'a Build,
    pub elf: ElfFile,
    pub targets: Vec<InstallTarget>,
    pub device: Target,
    /// The pack's name (`Russian`) and its files.
    pub language: Option<(String, Vec<LanguageFile>)>,
    pub starter_settings: bool,
    /// The user saw what is at a path and said to replace it.
    pub replace_foreign: bool,
    /// A newer build of the channel over the installed one (LDR-4).
    pub update: bool,
    /// Loaders a replacement at another path leaves behind, moved into the
    /// backup folder and out of the manifest (LDR-7): nothing is deleted.
    pub retire: Vec<String>,
    /// Unix seconds: names the backups, dates the manifest.
    pub now: i64,
}

fn rel(path: &str) -> io::Result<RelPath> {
    RelPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
}

/// `20261025-120000` for backup folders, from Unix seconds (UTC).
pub fn stamp(now: i64) -> String {
    let days = now.div_euclid(86_400);
    let secs = now.rem_euclid(86_400);
    // Civil date from days since 1970 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// The backup name of a path: `UPDT/OPL.ELF` → `UPDT_OPL.ELF`.
pub(crate) fn backup_name(path: &str) -> String {
    path.replace('/', "_")
}

/// Why `path` cannot take a loader, if it cannot.
pub(crate) fn refused(path: &str) -> Option<Phrase> {
    let parsed = RelPath::new(path).ok().filter(|p| !p.is_root());
    let Some(parsed) = parsed else {
        return Some(Phrase::notice("loaderPathInvalid").with("path", path));
    };
    if !path.to_ascii_lowercase().ends_with(".elf") {
        return Some(Phrase::notice("loaderPathInvalid").with("path", path));
    }
    let first = parsed.components().next().unwrap_or_default();
    if first.eq_ignore_ascii_case(".yaoplm")
        || EMULATOR_FILES.iter().any(|f| first.eq_ignore_ascii_case(f))
    {
        return Some(Phrase::notice("loaderPathReserved").with("path", path));
    }
    None
}

/// What sits at a loader's path.
enum Occupant {
    Empty,
    /// The very ELF being installed.
    Same,
    /// Another build of the family: replaced.
    Ours {
        len: u64,
    },
    /// Another family's program, or a file YAOPLM does not recognise.
    Foreign {
        len: u64,
        sha256: String,
    },
    Folder,
}

fn occupant(
    fs: &dyn DeviceFs,
    path: &RelPath,
    family: Family,
    sha256: &str,
) -> io::Result<Occupant> {
    let Some(parent) = path.parent() else {
        return Ok(Occupant::Empty);
    };
    let entries = list_or_empty(fs, &parent)?;
    let Some(entry) = find_entry(&entries, path.file_name()) else {
        return Ok(Occupant::Empty);
    };
    if entry.is_dir {
        return Ok(Occupant::Folder);
    }
    let real = parent
        .join(&entry.name)
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut bytes = Vec::new();
    fs.open(&real)?.take(MAX_READ + 1).read_to_end(&mut bytes)?;
    let len = entry.len;
    let sha = if bytes.len() as u64 <= MAX_READ {
        super::download::sha256(&bytes)
    } else {
        yaoplm_hash::sha256_reader(fs.open(&real)?)?
    };
    if sha == sha256 {
        return Ok(Occupant::Same);
    }
    let named = ps2_elf::program(&bytes)
        .ok()
        .and_then(|program| detect::program_family(&program));
    Ok(if named == Some(family) {
        Occupant::Ours { len }
    } else {
        Occupant::Foreign { len, sha256: sha }
    })
}

/// Folders `path` needs that are not there yet, outermost first.
pub(crate) fn missing_parents(
    fs: &dyn DeviceFs,
    path: &RelPath,
    planned: &mut Vec<String>,
) -> io::Result<Vec<Step>> {
    let mut steps = Vec::new();
    let mut walked = String::new();
    let parts: Vec<&str> = path.components().collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        if !walked.is_empty() {
            walked.push('/');
        }
        walked.push_str(part);
        let dir = rel(&walked)?;
        let exists = fs.metadata(&dir)?.is_some_and(|m| m.is_dir);
        if !exists && !planned.iter().any(|p| p.eq_ignore_ascii_case(&walked)) {
            planned.push(walked.clone());
            steps.push(Step::CreateDir { path: dir });
        }
    }
    Ok(steps)
}

/// Backups of `name` under [`BACKUP_DIR`], newest first: (folder, len).
pub(crate) fn backups(fs: &dyn DeviceFs, name: &str) -> io::Result<Vec<(String, u64)>> {
    let mut found = Vec::new();
    for stamp_dir in list_or_empty(fs, &rel(BACKUP_DIR)?)?
        .into_iter()
        .filter(|e| e.is_dir)
    {
        let dir = format!("{BACKUP_DIR}/{}", stamp_dir.name);
        if let Some(entry) =
            find_entry(&list_or_empty(fs, &rel(&dir)?)?, name).filter(|e| !e.is_dir)
        {
            found.push((dir, entry.len));
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(found)
}

/// A small file set through the plan: written whole, held to what was read.
pub(crate) fn put(fs: &dyn DeviceFs, path: &RelPath, content: Vec<u8>) -> io::Result<Option<Step>> {
    let current = fs
        .metadata(path)?
        .filter(|m| !m.is_dir)
        .map(|_| fs.read(path))
        .transpose()?;
    if current.as_deref() == Some(&content[..]) {
        return Ok(None);
    }
    Ok(Some(Step::PutFile {
        path: path.clone(),
        content: Bytes(content),
        expect: match current {
            Some(old) => Expect::Content(Bytes(old)),
            None => Expect::Absent,
        },
    }))
}

/// `title=` and `boot=` set in an app's `title.cfg`, its other keys kept.
fn title_cfg(
    fs: &dyn DeviceFs,
    path: &RelPath,
    title: &str,
    boot: &str,
) -> io::Result<Option<Step>> {
    let mut doc = match fs.metadata(path)? {
        Some(meta) if !meta.is_dir => CfgDocument::parse(&fs.read(path)?),
        _ => CfgDocument::new(),
    };
    let set = |doc: &mut CfgDocument, key: &str, value: &str| {
        doc.set(key, value)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
    };
    set(&mut doc, "title", title)?;
    set(&mut doc, "boot", boot)?;
    put(fs, path, doc.to_bytes())
}

/// Plans the install. A refused path or an unconfirmed foreign file shows
/// as an error notice: the plan cannot run until the request changes.
pub fn plan_install(fs: &dyn DeviceFs, request: &InstallRequest<'_>) -> io::Result<Plan> {
    let build = request.build;
    let state = DeviceState::read(fs)?;
    let mut plan = if request.update {
        new_plan(
            PlanKind::UpdateLoader,
            Phrase::title("updateLoader")
                .with("build", &build.name)
                .with("version", request.elf.version.as_deref().unwrap_or("?")),
            &state,
        )
    } else {
        new_plan(
            PlanKind::InstallLoader,
            Phrase::title("installLoader").with("build", &build.name),
            &state,
        )
    };
    plan.profile = Some(build.profile);
    plan.sources.push(Source {
        path: request.elf.path.clone(),
        kind: SourceKind::Raw,
        len: request.elf.len,
        files: vec![SourceFile {
            path: request.elf.path.clone(),
            len: request.elf.len,
            modified: request.elf.modified,
        }],
        on_device: false,
        check: None,
    });
    if request.targets.is_empty() {
        plan.notices
            .push(Notice::error(Phrase::notice("loaderNoPath")));
    }

    // 1. The build's folders.
    let mut planned: Vec<String> = Vec::new();
    let mut folders = Transaction::titled(Phrase::title("createFolders"));
    for name in build.layout.folders.iter().filter(|_| !request.update) {
        if !state.has_folder(name) {
            planned.push(name.clone());
            folders.push(Step::CreateDir { path: rel(name)? });
        }
    }
    if !folders.steps.is_empty() {
        plan.transactions.push(folders);
    }

    // 2. The ELF at each path.
    let stamp = stamp(request.now);
    // A manifest that does not read is replaced, but only as it was read.
    let manifest_path = rel(manifest::PATH)?;
    let manifest_raw = fs
        .metadata(&manifest_path)?
        .filter(|m| !m.is_dir)
        .map(|_| fs.read(&manifest_path))
        .transpose()?;
    let mut manifest = Manifest::read(fs).map(|(m, _)| m).unwrap_or_default();
    let mut installed_any = false;
    for target in &request.targets {
        if let Some(why) = refused(&target.path) {
            plan.notices.push(Notice::error(why));
            continue;
        }
        let path = rel(&target.path)?;
        let mut tx = Transaction::titled(
            Phrase::title("installLoaderAt")
                .with("build", &build.name)
                .with("path", &target.path),
        );
        tx.loader = true;
        match occupant(fs, &path, build.family, &request.elf.sha256)? {
            Occupant::Folder => {
                plan.notices.push(Notice::error(
                    Phrase::notice("loaderPathIsFolder").with("path", &target.path),
                ));
                continue;
            }
            Occupant::Same => {
                tx.notices.push(Notice::info(
                    Phrase::notice("loaderAlreadyThere").with("path", &target.path),
                ));
            }
            occupant => {
                for step in missing_parents(fs, &path, &mut planned)? {
                    tx.push(step);
                }
                let replaced = match occupant {
                    Occupant::Ours { len } => Some(len),
                    Occupant::Foreign { len, sha256 } => {
                        let phrase = Phrase::notice("loaderForeignFile")
                            .with("path", &target.path)
                            .with("size", len)
                            .with("sha256", sha256);
                        if request.replace_foreign {
                            tx.notices.push(Notice::warning(phrase));
                        } else {
                            plan.notices.push(Notice::error(phrase));
                        }
                        Some(len)
                    }
                    _ => None,
                };
                let name = backup_name(&target.path);
                let mut stale = Vec::new();
                if replaced.is_some() {
                    let dir = format!("{BACKUP_DIR}/{stamp}");
                    let backup = rel(&format!("{dir}/{name}"))?;
                    for step in missing_parents(fs, &backup, &mut planned)? {
                        tx.push(step);
                    }
                    tx.push(Step::Rename {
                        from: path.clone(),
                        to: backup,
                    });
                    stale = backups(fs, &name)?
                        .into_iter()
                        .filter(|(folder, _)| *folder != dir)
                        .skip(KEEP_BACKUPS - 1)
                        .collect();
                }
                tx.push(Step::WriteFile {
                    path: path.clone(),
                    source: 0,
                    offset: 0,
                    len: request.elf.len,
                });
                // After the commit point: only the backups the rule drops.
                for (folder, len) in stale {
                    tx.then(Step::Remove {
                        path: rel(&format!("{folder}/{name}"))?,
                        len,
                        what: Removal::Backup,
                    });
                    tx.then(Step::RemoveDir {
                        path: rel(&folder)?,
                    });
                }
            }
        }
        if let Some(title) = target.title.as_ref().filter(|_| !request.update)
            && let Some(parent) = path.parent()
        {
            let cfg = parent
                .join("title.cfg")
                .map_err(|e| io::Error::other(e.to_string()))?;
            if let Some(step) = title_cfg(fs, &cfg, title, path.file_name())? {
                if tx.commit == tx.steps.len() {
                    tx.push(step);
                } else {
                    tx.then(step);
                }
            }
        }
        manifest.put(Installed {
            build: build.id.clone(),
            tag: request.elf.tag.clone(),
            version: request.elf.version.clone(),
            sha256: request.elf.sha256.clone(),
            path: target.path.clone(),
            preset: target.preset.clone(),
            device: match request.device {
                Target::Usb => "usb".into(),
                Target::Mx4sio => "mx4sio".into(),
            },
            language: request.language.as_ref().map(|(name, _)| name.clone()),
            source: request.elf.source.clone(),
            installed_at: request.now,
        });
        installed_any = true;
        if !tx.steps.is_empty() || !tx.notices.is_empty() {
            plan.transactions.push(tx);
        }
    }

    // The loaders a replacement leaves behind, once the new one is in place.
    for path in &request.retire {
        if request
            .targets
            .iter()
            .any(|t| t.path.eq_ignore_ascii_case(path))
        {
            continue;
        }
        if let Some(why) = refused(path) {
            plan.notices.push(Notice::error(why));
            continue;
        }
        let target = rel(path)?;
        let Some(parent) = target.parent() else {
            continue;
        };
        let entries = list_or_empty(fs, &parent)?;
        let Some(entry) = find_entry(&entries, target.file_name()).filter(|e| !e.is_dir) else {
            continue;
        };
        let current = parent
            .join(&entry.name)
            .map_err(|e| io::Error::other(e.to_string()))?;
        let name = backup_name(path);
        let dir = format!("{BACKUP_DIR}/{stamp}");
        let backup = rel(&format!("{dir}/{name}"))?;
        let mut tx = Transaction::titled(Phrase::title("retireLoader").with("path", path));
        tx.loader = true;
        for step in missing_parents(fs, &backup, &mut planned)? {
            tx.push(step);
        }
        tx.push(Step::Rename {
            from: current,
            to: backup,
        });
        for (folder, len) in backups(fs, &name)?
            .into_iter()
            .filter(|(folder, _)| *folder != dir)
            .skip(KEEP_BACKUPS - 1)
        {
            tx.then(Step::Remove {
                path: rel(&format!("{folder}/{name}"))?,
                len,
                what: Removal::Backup,
            });
            tx.then(Step::RemoveDir {
                path: rel(&folder)?,
            });
        }
        manifest.remove(path);
        plan.transactions.push(tx);
    }

    // 3. The language pack.
    if let Some((language, files)) = &request.language
        && let Some(folder) = &build.layout.languages
    {
        let mut tx =
            Transaction::titled(Phrase::title("installLanguage").with("language", language));
        tx.loader = true;
        if !state.has_folder(folder) && !planned.iter().any(|p| p.eq_ignore_ascii_case(folder)) {
            tx.push(Step::CreateDir { path: rel(folder)? });
        }
        for file in files {
            if let Some(step) = put(
                fs,
                &rel(&format!("{folder}/{}", file.name))?,
                file.bytes.clone(),
            )? {
                tx.push(step);
            }
        }
        if !tx.steps.is_empty() {
            plan.transactions.push(tx);
        }
    }

    // 4. The manifest, once the ELFs are in place.
    if installed_any {
        let mut tx = Transaction::titled(Phrase::title("writeManifest"));
        for step in missing_parents(fs, &manifest_path, &mut planned)? {
            tx.push(step);
        }
        let content = manifest.to_bytes();
        if manifest_raw.as_deref() != Some(&content[..]) {
            tx.push(Step::PutFile {
                path: manifest_path,
                content: Bytes(content),
                expect: match manifest_raw {
                    Some(old) => Expect::Content(Bytes(old)),
                    None => Expect::Absent,
                },
            });
            plan.transactions.push(tx);
        }
    }

    // 5. Starter settings (ADR-0042): USB only; the card's copy wins.
    if request.starter_settings && !request.update {
        match request.device {
            Target::Mx4sio => plan
                .notices
                .push(Notice::info(Phrase::notice("starterSettingsMx4sio"))),
            Target::Usb => {
                let path = rel(&build.layout.settings)?;
                let mut doc = match fs.metadata(&path)? {
                    Some(meta) if !meta.is_dir => CfgDocument::parse(&fs.read(&path)?),
                    _ => CfgDocument::new(),
                };
                let mut set = |key: &str, value: &str| {
                    if build.layout.seed.iter().any(|k| k == key) {
                        doc.set(key, value)
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
                    } else {
                        Ok(())
                    }
                };
                set("usb_mode", "2")?;
                if let Some((language, _)) = &request.language {
                    set("language_text", language)?;
                }
                if let Some(step) = put(fs, &path, doc.to_bytes())? {
                    let mut tx = Transaction::titled(Phrase::title("writeStarterSettings"));
                    tx.loader = true;
                    tx.push(step);
                    tx.notices
                        .push(Notice::info(Phrase::notice("starterSettingsCard")));
                    plan.transactions.push(tx);
                }
            }
        }
    }
    if plan.notices.iter().all(|n| n.level != Level::Error) && plan.transactions.is_empty() {
        plan.notices
            .push(Notice::info(Phrase::notice("loaderNothingToDo")));
    }
    Ok(plan)
}
