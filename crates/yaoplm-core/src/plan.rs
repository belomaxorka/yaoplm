//! Plans: every write YAOPLM makes, described before it happens (IMP-10).
//!
//! A [`Plan`] is a list of [`Transaction`]s, one per game (or per folder
//! set-up, rename, delete...). Each transaction is a list of [`Step`]s with a
//! commit point: the steps before it can be undone, the step just before it
//! makes the change visible to OPL (an image renamed into place, `ul.cfg`
//! rewritten), and the steps after it only finish the job (removing
//! `games.bin`, deleting what the user asked to delete). A crash before the
//! commit is rolled back, a crash after it is completed (research/02 §5,
//! SPEC §6 "Безопасность данных").
//!
//! Plans are plain data: planners build them from what is on the device,
//! [`crate::exec`] runs them, and the journal stores them as JSON.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use yaoplm_device::RelPath;
use yaoplm_formats::Media;
use yaoplm_i18n::Phrase;

/// How a source turns into image bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceKind {
    /// Copied byte for byte: an ISO, or a ZSO kept compressed.
    Raw,
    /// The ISO sectors inside a ZSO.
    Zso,
    /// The 2048 user-data bytes of each raw sector of a single-track BIN.
    #[serde(rename_all = "camelCase")]
    Bin {
        sector_size: u32,
        data_offset: u32,
        /// `INDEX 01` of the track.
        first_sector: u64,
        sectors: u64,
    },
    /// `files` read one after the other: the `ul` chunks of a game being
    /// converted into one image (GAME-4).
    Joined,
    /// The ISO sectors of a CHD (IMP-1b).
    Chd,
}

/// A file a source reads, as it was when the plan was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<u128>,
}

/// An import source outside the device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    /// The file the bytes come from (the BIN for a CUE sheet).
    pub path: PathBuf,
    pub kind: SourceKind,
    /// Bytes the source yields: the ISO size for ZSO and BIN sources.
    pub len: u64,
    /// Everything that belongs to the source (the CUE, then its BIN: the
    /// data file is always last), checked for changes before execution and
    /// removed when moving.
    pub files: Vec<SourceFile>,
    /// The source is on the device itself (a conversion, GAME-4): `path`
    /// and the `files` are paths relative to the root, read through the
    /// device and never removed through `RemoveSource`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_device: bool,
}

/// Small file contents, stored as hex in the journal.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", try_from = "String")]
pub struct Bytes(pub Vec<u8>);

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytes({} bytes)", self.0.len())
    }
}

impl From<Bytes> for String {
    fn from(bytes: Bytes) -> Self {
        bytes.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl TryFrom<String> for Bytes {
    type Error = String;

    fn try_from(hex: String) -> Result<Self, String> {
        if !hex.len().is_multiple_of(2) {
            return Err("odd hex length".into());
        }
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|e| e.to_string()))
            .collect::<Result<Vec<u8>, String>>()
            .map(Bytes)
    }
}

/// What a small-file write expects to find.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
pub enum Expect {
    Absent,
    Content(Bytes),
    /// Recovery only: whatever is there is replaced.
    Any,
}

/// Why a file is removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Removal {
    /// The user's data: an image, chunks, CFG, art. Shown highlighted and
    /// only after an explicit confirmation.
    UserData,
    /// OPL's `games.bin`, rebuilt by OPL on the next scan (IMP-8).
    Cache,
    /// A file YAOPLM created itself in this operation (rollback).
    Ours,
    /// A ZSO this operation compressed (rollback of `WriteZso`, whose size
    /// is not known up front): removed only when its header says it holds
    /// `iso_len` bytes, so a file someone else put there stays.
    #[serde(rename_all = "camelCase")]
    OursZso { iso_len: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Step {
    /// A folder (LIB-3).
    CreateDir {
        path: RelPath,
    },
    /// Removes a folder if it is empty (rollback of `CreateDir`).
    RemoveDir {
        path: RelPath,
    },
    /// A new file with `len` bytes of source `source` from `offset`: an
    /// image, or one `ul` chunk. Written as `<path>.yaoplm-part`, synced,
    /// then renamed into place.
    WriteFile {
        path: RelPath,
        source: usize,
        offset: u64,
        len: u64,
    },
    /// A new ZSO of the ISO sectors of source `source` (GAME-4). Its size is
    /// known only once written; `max_len` is the worst case, reserved for
    /// the space check. Written as `<path>.yaoplm-part`, synced, renamed.
    WriteZso {
        path: RelPath,
        source: usize,
        max_len: u64,
    },
    /// Replaces a small file atomically (`ul.cfg`, a CFG).
    PutFile {
        path: RelPath,
        content: Bytes,
        expect: Expect,
    },
    /// Copies a small file inside the device: a CFG backup (CFG-5).
    CopyFile {
        from: RelPath,
        to: RelPath,
    },
    Rename {
        from: RelPath,
        to: RelPath,
    },
    Remove {
        path: RelPath,
        len: u64,
        what: Removal,
    },
    /// Deletes an import source after a move (IMP-11).
    RemoveSource {
        source: usize,
    },
    /// Deletes a finished journal (recovery).
    DropJournal {
        id: String,
    },
}

impl Step {
    /// Steps that cannot be undone must come after the commit point.
    pub fn is_undoable(&self) -> bool {
        !matches!(
            self,
            Self::Remove { .. } | Self::RemoveSource { .. } | Self::DropJournal { .. }
        ) && !matches!(
            self,
            Self::PutFile {
                expect: Expect::Any,
                ..
            }
        )
    }

    /// Bytes this step writes to the device (at most).
    pub fn write_bytes(&self) -> u64 {
        match self {
            Self::WriteFile { len, .. } => *len,
            Self::WriteZso { max_len, .. } => *max_len,
            Self::PutFile { content, .. } => content.0.len() as u64,
            _ => 0,
        }
    }

    /// Bytes of the user's files this step deletes on the device.
    pub fn freed_bytes(&self) -> u64 {
        match self {
            Self::Remove {
                len,
                what: Removal::UserData,
                ..
            } => *len,
            _ => 0,
        }
    }
}

/// The game a transaction is about, for display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInfo {
    pub title: String,
    pub startup: String,
    #[serde(with = "media_name")]
    pub media: Media,
    /// `ISO`, `ZSO` or `UL`.
    pub format: String,
}

/// `Media` as `"CD"` / `"DVD"`; `yaoplm-formats` stays free of serde.
mod media_name {
    use serde::{Deserialize, Deserializer, Serializer};
    use yaoplm_formats::Media;

    pub fn serialize<S: Serializer>(media: &Media, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(media.folder())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Media, D::Error> {
        match String::deserialize(d)?.as_str() {
            "CD" => Ok(Media::Cd),
            "DVD" => Ok(Media::Dvd),
            other => Err(serde::de::Error::custom(format!("unknown media {other}"))),
        }
    }
}

/// One unit that either happens completely or not at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// English, from [`Transaction::title_phrase`] when there is one.
    pub title: String,
    /// For front ends that translate the title; journals written before
    /// it existed have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_phrase: Option<Phrase>,
    pub game: Option<GameInfo>,
    pub steps: Vec<Step>,
    /// Steps `..commit` are undoable; `commit..` only finish the job.
    pub commit: usize,
    pub notices: Vec<Notice>,
}

impl Transaction {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            title_phrase: None,
            game: None,
            steps: Vec::new(),
            commit: 0,
            notices: Vec::new(),
        }
    }

    pub fn titled(title: Phrase) -> Self {
        let mut tx = Self::new(String::new());
        tx.set_title(title);
        tx
    }

    pub fn set_title(&mut self, title: Phrase) {
        self.title = title.english();
        self.title_phrase = Some(title);
    }

    /// Adds an undoable step before the commit point.
    pub fn push(&mut self, step: Step) {
        debug_assert!(step.is_undoable(), "{step:?} must come after the commit");
        debug_assert_eq!(self.commit, self.steps.len(), "commit already set");
        self.steps.push(step);
        self.commit = self.steps.len();
    }

    /// Adds a step after the commit point.
    pub fn then(&mut self, step: Step) {
        self.steps.push(step);
    }

    pub fn write_bytes(&self) -> u64 {
        self.steps.iter().map(Step::write_bytes).sum()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    Info,
    Warning,
    /// The plan (or this item) cannot be executed.
    Error,
}

/// Something the user should read before confirming. `code` is stable (the
/// UI and the planners key off it); `phrase` says it in the user's
/// language (`notices.<code>`, ADR-0029) and `message` is its English.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    pub level: Level,
    pub code: String,
    pub message: String,
    /// Notices in journals written before phrases existed have none.
    /// Boxed: a notice travels inside every skip result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<Box<Phrase>>,
}

impl Notice {
    /// A notice saying `phrase`, whose key is `notices.<code>`.
    pub fn new(level: Level, phrase: Phrase) -> Self {
        let code = phrase.key.rsplit('.').next().unwrap_or_default().to_owned();
        Self {
            level,
            code,
            message: phrase.english(),
            phrase: Some(Box::new(phrase)),
        }
    }

    /// What the notice says, for a front end or inside another phrase;
    /// a notice from an old journal passes its English through.
    pub fn said(&self) -> Phrase {
        self.phrase
            .as_deref()
            .cloned()
            .unwrap_or_else(|| Phrase::reason("text").with("text", &self.message))
    }

    pub fn info(phrase: Phrase) -> Self {
        Self::new(Level::Info, phrase)
    }

    pub fn warning(phrase: Phrase) -> Self {
        Self::new(Level::Warning, phrase)
    }

    pub fn error(phrase: Phrase) -> Self {
        Self::new(Level::Error, phrase)
    }
}

/// An input that will not be imported, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: Notice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanKind {
    InitLibrary,
    Import,
    Rename,
    Delete,
    Move,
    Recovery,
    /// Cover art written or removed (ART-1, ART-3, ART-4).
    Art,
    /// Games converted between ISO, ZSO and `ul` (GAME-4).
    Convert,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub kind: PlanKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_phrase: Option<Phrase>,
    /// The device as it was when planned ([`yaoplm_device::FsInfo::fingerprint`]).
    pub fingerprint: String,
    pub sources: Vec<Source>,
    pub transactions: Vec<Transaction>,
    pub skipped: Vec<Skipped>,
    /// Plan-wide notices. Any [`Level::Error`] here blocks execution.
    pub notices: Vec<Notice>,
    pub free_bytes: u64,
}

impl Plan {
    pub fn new(
        kind: PlanKind,
        title: impl Into<String>,
        fingerprint: String,
        free_bytes: u64,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            title_phrase: None,
            fingerprint,
            sources: Vec::new(),
            transactions: Vec::new(),
            skipped: Vec::new(),
            notices: Vec::new(),
            free_bytes,
        }
    }

    pub fn titled(kind: PlanKind, title: Phrase, fingerprint: String, free_bytes: u64) -> Self {
        let mut plan = Self::new(kind, String::new(), fingerprint, free_bytes);
        plan.set_title(title);
        plan
    }

    pub fn set_title(&mut self, title: Phrase) {
        self.title = title.english();
        self.title_phrase = Some(title);
    }

    pub fn write_bytes(&self) -> u64 {
        self.transactions.iter().map(Transaction::write_bytes).sum()
    }

    /// The most free space the plan needs at any moment. Transactions run
    /// in order and a conversion deletes the old image at the end of its
    /// own transaction, so what it frees is there for the next one.
    pub fn peak_write_bytes(&self) -> u64 {
        let mut running = 0u64;
        let mut peak = 0u64;
        for tx in &self.transactions {
            running += tx.write_bytes();
            peak = peak.max(running);
            let freed: u64 = tx.steps.iter().map(Step::freed_bytes).sum();
            running = running.saturating_sub(freed);
        }
        peak
    }

    /// Bytes of the user's files this plan deletes (shown highlighted).
    pub fn delete_bytes(&self) -> u64 {
        self.transactions
            .iter()
            .flat_map(|t| &t.steps)
            .map(|step| match step {
                Step::Remove {
                    len,
                    what: Removal::UserData,
                    ..
                } => *len,
                Step::RemoveSource { source } => self
                    .sources
                    .get(*source)
                    .map_or(0, |s| s.files.iter().map(|f| f.len).sum()),
                _ => 0,
            })
            .sum()
    }

    /// Errors that keep the plan from running.
    pub fn blockers(&self) -> impl Iterator<Item = &Notice> {
        self.notices.iter().filter(|n| n.level == Level::Error)
    }

    pub fn is_executable(&self) -> bool {
        self.blockers().next().is_none() && !self.transactions.is_empty()
    }

    /// True when executing deletes the user's files or import sources.
    pub fn deletes_user_data(&self) -> bool {
        self.transactions.iter().flat_map(|t| &t.steps).any(|s| {
            matches!(
                s,
                Step::Remove {
                    what: Removal::UserData,
                    ..
                } | Step::RemoveSource { .. }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(path: &str) -> RelPath {
        RelPath::new(path).unwrap()
    }

    #[test]
    fn steps_round_trip_as_json() {
        let steps = vec![
            Step::CreateDir { path: rel("CD") },
            Step::WriteFile {
                path: rel("CD/a.iso"),
                source: 0,
                offset: 1 << 30,
                len: 2048,
            },
            Step::PutFile {
                path: rel("ul.cfg"),
                content: Bytes(vec![0, 0x14, 0xff]),
                expect: Expect::Content(Bytes(vec![1])),
            },
            Step::Remove {
                path: rel("CD/games.bin"),
                len: 188,
                what: Removal::Cache,
            },
        ];
        let json = serde_json::to_string(&steps).unwrap();
        assert!(json.contains(r#""content":"0014ff""#), "{json}");
        assert!(json.contains(r#""op":"writeFile""#), "{json}");
        let back: Vec<Step> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, steps);
        // Paths are validated again when read back.
        let bad = json.replace("CD/a.iso", "../a.iso");
        assert!(serde_json::from_str::<Vec<Step>>(&bad).is_err());
    }

    #[test]
    fn commit_point_and_totals() {
        let mut tx = Transaction::new("t");
        tx.push(Step::CreateDir { path: rel("DVD") });
        tx.push(Step::WriteFile {
            path: rel("DVD/a.iso"),
            source: 0,
            offset: 0,
            len: 4096,
        });
        tx.then(Step::Remove {
            path: rel("DVD/games.bin"),
            len: 10,
            what: Removal::Cache,
        });
        assert_eq!(tx.commit, 2);
        let mut plan = Plan::new(PlanKind::Import, "p", "fp".into(), 1 << 30);
        plan.transactions.push(tx);
        assert_eq!(plan.write_bytes(), 4096);
        assert_eq!(plan.delete_bytes(), 0);
        assert!(plan.is_executable());
        assert!(!plan.deletes_user_data());
        plan.notices.push(Notice::error(
            Phrase::notice("noSpace")
                .with("needed", 2u64)
                .with("free", 1u64),
        ));
        assert!(!plan.is_executable());
    }
}
