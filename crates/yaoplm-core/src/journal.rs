//! The journal of a running plan, kept on the device in `.yaoplm/journal/`
//! (IMP-5, SPEC §6).
//!
//! It holds the whole plan plus, per transaction, how many steps are done.
//! It is rewritten atomically after every step and deleted when the plan
//! has finished cleanly, so a journal that is still there after a crash or
//! an unplugged stick says exactly what to undo or finish ([`crate::recovery`]).
//! OPL never lists the root, so the folder is invisible on the console.

use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use yaoplm_device::RelPath;
use yaoplm_device::fs::{DeviceFs, TMP_SUFFIX, list_or_empty, write_atomic};

use crate::plan::Plan;

pub const DATA_DIR: &str = ".yaoplm";
pub const JOURNAL_DIR: &str = ".yaoplm/journal";
pub const BACKUP_DIR: &str = ".yaoplm/backup";
pub const VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TxState {
    Pending,
    /// Started, commit point not reached: roll back or redo.
    Running,
    /// Past the commit point: only finishing is possible.
    Committed,
    Done,
    RolledBack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxProgress {
    pub state: TxState,
    /// Steps completed.
    pub done: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub version: u32,
    pub id: String,
    /// Seconds since 1970.
    pub created: u64,
    pub plan: Plan,
    pub progress: Vec<TxProgress>,
}

impl Journal {
    pub fn new(plan: &Plan) -> Self {
        Self {
            version: VERSION,
            id: new_id(),
            created: now(),
            plan: plan.clone(),
            progress: vec![
                TxProgress {
                    state: TxState::Pending,
                    done: 0,
                };
                plan.transactions.len()
            ],
        }
    }

    /// True when nothing on the device is half done.
    pub fn is_settled(&self) -> bool {
        self.progress.iter().all(|p| {
            matches!(
                p.state,
                TxState::Pending | TxState::Done | TxState::RolledBack
            )
        })
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `20261022-235959-1a2b3c`: sortable, unique within a process.
pub fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    let unique = (u64::from(since.subsec_nanos()) ^ u64::from(std::process::id()) << 16)
        .wrapping_add(COUNTER.fetch_add(1, Ordering::Relaxed));
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}-{:06x}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        unique & 0xFF_FFFF
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn rel(path: &str) -> io::Result<RelPath> {
    RelPath::new(path).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

pub fn path(id: &str) -> io::Result<RelPath> {
    rel(JOURNAL_DIR)?
        .join(&format!("{id}.json"))
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
}

pub fn save(fs: &dyn DeviceFs, journal: &Journal) -> io::Result<()> {
    fs.create_dir(&rel(DATA_DIR)?)?;
    fs.create_dir(&rel(JOURNAL_DIR)?)?;
    let json = serde_json::to_vec_pretty(journal).map_err(io::Error::other)?;
    write_atomic(fs, &path(&journal.id)?, &json)
}

pub fn remove(fs: &dyn DeviceFs, id: &str) -> io::Result<()> {
    match fs.remove_file(&path(id)?) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        other => other?,
    }
    // Nothing of YAOPLM's stays behind once the operation is done: a whole
    // system disk shared over SMB must not keep `.yaoplm/` (ADR-0044). The
    // folders go only when empty; backups and other journals keep them.
    for dir in [JOURNAL_DIR, DATA_DIR] {
        let dir = rel(dir)?;
        if list_or_empty(fs, &dir)?.is_empty() && fs.metadata(&dir)?.is_some() {
            let _ = fs.remove_dir(&dir);
        }
    }
    Ok(())
}

/// Removes temporary files of journal writes that never completed (the
/// device vanished mid-write). They are never a journal: an atomic write
/// leaves the previous `.json`, if any, in place.
pub fn clean_temp(fs: &dyn DeviceFs) -> io::Result<()> {
    let dir = rel(JOURNAL_DIR)?;
    for entry in list_or_empty(fs, &dir)? {
        if !entry.is_dir && entry.name.ends_with(TMP_SUFFIX) {
            let path = dir
                .join(&entry.name)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            fs.remove_file(&path)?;
        }
    }
    Ok(())
}

/// A journal file that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unreadable {
    pub file: String,
    pub error: String,
}

/// Every journal left on the device, oldest first.
pub fn load_all(fs: &dyn DeviceFs) -> io::Result<(Vec<Journal>, Vec<Unreadable>)> {
    let mut journals = Vec::new();
    let mut unreadable = Vec::new();
    for entry in list_or_empty(fs, &rel(JOURNAL_DIR)?)? {
        if entry.is_dir || !entry.name.ends_with(".json") {
            continue;
        }
        let file = rel(JOURNAL_DIR)?
            .join(&entry.name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let parsed = fs
            .read(&file)
            .map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice::<Journal>(&bytes).map_err(|e| e.to_string()));
        match parsed {
            Ok(journal) => journals.push(journal),
            Err(error) => unreadable.push(Unreadable {
                file: entry.name,
                error,
            }),
        }
    }
    journals.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((journals, unreadable))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_dated_and_unique() {
        let (a, b) = (new_id(), new_id());
        assert_ne!(a, b);
        assert_eq!(a.len(), "20261022-235959-1a2b3c".len());
        assert!(a.starts_with("20"), "{a}");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_748), (2026, 10, 22));
    }
}
