//! Rolling a loader back (SPEC LDR-4, §5.10.1, ADR-0045): the newest backup
//! of a path goes back in place, and the file it replaces becomes the
//! newest backup, so a second rollback undoes the first. The manifest
//! follows what is back: a catalogue build keeps its entry, another program
//! loses it. The language pack stays: `LNG/` holds one per device.

use std::io;

use yaoplm_device::fs::{find_entry, list_or_empty};
use yaoplm_device::{DeviceFs, RelPath};
use yaoplm_i18n::Phrase;

use super::Catalog;
use super::install::{BACKUP_DIR, backup_name, missing_parents, refused, stamp};
use super::installed::{newest_backup, read_program};
use super::manifest::{self, Installed, Manifest};
use crate::device::DeviceState;
use crate::ops::new_plan;
use crate::plan::{Bytes, Expect, Notice, Plan, PlanKind, Step, Transaction};

fn rel(path: &str) -> io::Result<RelPath> {
    RelPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.to_string()))
}

/// Plans putting the newest backup of `path` back. No backup is an error
/// notice: the plan cannot run.
pub fn plan_rollback(
    fs: &dyn DeviceFs,
    path: &str,
    catalog: &Catalog,
    now: i64,
) -> io::Result<Plan> {
    let state = DeviceState::read(fs)?;
    let mut plan = new_plan(
        PlanKind::RollbackLoader,
        Phrase::title("rollbackLoader").with("path", path),
        &state,
    );
    if let Some(why) = refused(path) {
        plan.notices.push(Notice::error(why));
        return Ok(plan);
    }
    let Some(backup) = newest_backup(fs, path)? else {
        plan.notices.push(Notice::error(
            Phrase::notice("loaderNoBackup").with("path", path),
        ));
        return Ok(plan);
    };
    let target = rel(path)?;
    let parent = target.parent().unwrap_or_else(RelPath::root);
    let current = find_entry(&list_or_empty(fs, &parent)?, target.file_name())
        .filter(|e| !e.is_dir)
        .map(|e| parent.join(&e.name))
        .transpose()
        .map_err(|e| io::Error::other(e.to_string()))?;
    let backup_file = rel(&backup.file)?;
    let backup_folder = backup_file.parent().unwrap_or_else(RelPath::root);

    let mut tx = Transaction::titled(
        Phrase::title("restoreLoader")
            .with("path", path)
            .with("backup", &backup.file),
    );
    let mut planned = Vec::new();
    // What is there now becomes the newest backup; a second rollback
    // brings it back.
    if let Some(current) = &current {
        let mut folder = format!("{BACKUP_DIR}/{}", stamp(now));
        if folder.eq_ignore_ascii_case(backup_folder.as_str()) {
            folder.push_str("-1");
        }
        let kept = rel(&format!("{folder}/{}", backup_name(path)))?;
        for step in missing_parents(fs, &kept, &mut planned)? {
            tx.push(step);
        }
        tx.push(Step::Rename {
            from: current.clone(),
            to: kept,
        });
    } else {
        for step in missing_parents(fs, &target, &mut planned)? {
            tx.push(step);
        }
    }
    tx.push(Step::Rename {
        from: backup_file.clone(),
        to: current.clone().unwrap_or(target),
    });
    tx.then(Step::RemoveDir {
        path: backup_folder,
    });
    plan.transactions.push(tx);

    // The manifest names what is back, if it is a catalogue build.
    let manifest_path = rel(manifest::PATH)?;
    let raw = fs
        .metadata(&manifest_path)?
        .filter(|m| !m.is_dir)
        .map(|_| fs.read(&manifest_path))
        .transpose()?;
    let mut manifest = Manifest::read(fs).map(|(m, _)| m).unwrap_or_default();
    let before = manifest.entry(path).cloned();
    let (family, version, sha256) = read_program(fs, &backup_file)?;
    let build = family
        .zip(version.as_deref())
        .and_then(|(family, version)| {
            let version = super::BuildVersion::parse(version)?;
            catalog.channel_of_version(family, &version)
        });
    match build {
        Some(build) => manifest.put(Installed {
            build: build.id.clone(),
            tag: None,
            version: version.clone(),
            sha256,
            path: path.to_owned(),
            preset: before.as_ref().and_then(|e| e.preset.clone()),
            device: before
                .as_ref()
                .map_or_else(|| "usb".to_owned(), |e| e.device.clone()),
            language: before.as_ref().and_then(|e| e.language.clone()),
            source: backup.file.clone(),
            installed_at: now,
        }),
        None => manifest.remove(path),
    }
    let content = manifest.to_bytes();
    let unchanged = match &raw {
        Some(old) => *old == content,
        // No manifest and nothing to name in one.
        None => manifest.loaders.is_empty(),
    };
    if !unchanged {
        let mut tx = Transaction::titled(Phrase::title("writeManifest"));
        for step in missing_parents(fs, &manifest_path, &mut planned)? {
            tx.push(step);
        }
        tx.push(Step::PutFile {
            path: manifest_path,
            content: Bytes(content),
            expect: match raw {
                Some(old) => Expect::Content(Bytes(old)),
                None => Expect::Absent,
            },
        });
        plan.transactions.push(tx);
    }
    Ok(plan)
}
