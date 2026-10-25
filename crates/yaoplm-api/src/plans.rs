//! Plans on the wire, and the store that keeps them between "plan" and
//! "execute" (IMP-10).
//!
//! Front ends only ever see a [`PlanView`] and send back its id: the plan
//! that runs is the one the core built, not something a client edited. A
//! plan runs at most once and expires after an hour.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_core::art::ArtItem;
use yaoplm_core::plan::{self as core, Level, Removal, Step};
use yaoplm_device::RelPath;

use crate::library::{GameFormat, Media};
use crate::paths::display_path;
use crate::phrase::Phrase;
use crate::{ApiError, ErrorCode};

const MAX_PLANS: usize = 32;
const PLAN_TTL: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum NoticeLevel {
    Info,
    Warning,
    /// Blocks the plan (or skips that item).
    Error,
}

/// A notice with a level: `code` is stable, `message` is English and
/// `phrase` says it in the front end's language (ADR-0029).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct PlanNotice {
    pub level: NoticeLevel,
    pub code: String,
    pub message: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phrase: Option<Phrase>,
}

impl From<&core::Notice> for PlanNotice {
    fn from(notice: &core::Notice) -> Self {
        Self {
            level: match notice.level {
                Level::Info => NoticeLevel::Info,
                Level::Warning => NoticeLevel::Warning,
                Level::Error => NoticeLevel::Error,
            },
            code: notice.code.clone(),
            message: notice.message.clone(),
            phrase: notice.phrase.as_deref().map(Phrase::from),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum PlanKind {
    InitLibrary,
    Import,
    Rename,
    Delete,
    Move,
    Recovery,
    Art,
    Convert,
    /// Game settings (CFG-2).
    Settings,
    /// Memory cards (VMC-1).
    Vmc,
    /// A loader build put on the device (LDR-3).
    InstallLoader,
    /// A newer build of the installed loader's channel (LDR-4).
    UpdateLoader,
    /// A loader's backup put back (LDR-4).
    RollbackLoader,
    /// Another build in place of the installed one (LDR-7).
    ReplaceLoader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum StepOp {
    CreateDir,
    RemoveDir,
    WriteFile,
    /// Writes a ZSO compressed from the source (GAME-4); `bytes` is the
    /// most it can take.
    WriteZso,
    /// Formats a blank memory card (VMC-1).
    WriteVmc,
    /// Replaces a small file atomically (`ul.cfg`, a CFG).
    PutFile,
    CopyFile,
    Rename,
    Remove,
    /// Deletes an import source (move mode).
    RemoveSource,
    DropJournal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum RemovalKind {
    /// The user's files: highlight, and require `confirmDelete`.
    UserData,
    /// OPL's `games.bin` cache.
    Cache,
    /// Leftovers of this operation.
    Ours,
    /// An older backup of a replaced loader (the newest three stay).
    Backup,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub op: StepOp,
    /// Device path relative to the root, or a host path for sources.
    pub path: String,
    pub to: Option<String>,
    pub bytes: Option<u64>,
    pub removal: Option<RemovalKind>,
    /// For writes: the file the bytes come from.
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlanGame {
    pub title: String,
    pub startup: String,
    pub media: Media,
    pub format: GameFormat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlanTransaction {
    pub title: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_phrase: Option<Phrase>,
    pub game: Option<PlanGame>,
    pub steps: Vec<PlanStep>,
    /// Steps before this index are undone if anything fails.
    pub commit: u32,
    pub notices: Vec<PlanNotice>,
    /// CFG keys this transaction changes (CFG-2).
    #[serde(default)]
    pub cfg_changes: Vec<CfgKeyChange>,
}

/// A CFG key before and after a plan; `None` is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct CfgKeyChange {
    pub key: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlanSkipped {
    pub path: String,
    pub reason: PlanNotice,
}

/// What a plan will do, for the confirmation screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    /// Pass to `ExecutePlan`.
    pub id: String,
    pub kind: PlanKind,
    pub title: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_phrase: Option<Phrase>,
    pub root: String,
    pub write_bytes: u64,
    /// The user's files this plan deletes.
    pub delete_bytes: u64,
    pub free_bytes: u64,
    pub executable: bool,
    /// `ExecutePlan` then needs `confirmDelete: true`.
    pub deletes_user_data: bool,
    pub transactions: Vec<PlanTransaction>,
    pub skipped: Vec<PlanSkipped>,
    pub notices: Vec<PlanNotice>,
}

fn kind(kind: core::PlanKind) -> PlanKind {
    match kind {
        core::PlanKind::InitLibrary => PlanKind::InitLibrary,
        core::PlanKind::Import => PlanKind::Import,
        core::PlanKind::Rename => PlanKind::Rename,
        core::PlanKind::Delete => PlanKind::Delete,
        core::PlanKind::Move => PlanKind::Move,
        core::PlanKind::Recovery => PlanKind::Recovery,
        core::PlanKind::Art => PlanKind::Art,
        core::PlanKind::Convert => PlanKind::Convert,
        core::PlanKind::Settings => PlanKind::Settings,
        core::PlanKind::Vmc => PlanKind::Vmc,
        core::PlanKind::InstallLoader => PlanKind::InstallLoader,
        core::PlanKind::UpdateLoader => PlanKind::UpdateLoader,
        core::PlanKind::RollbackLoader => PlanKind::RollbackLoader,
        core::PlanKind::ReplaceLoader => PlanKind::ReplaceLoader,
    }
}

fn format(format: &str) -> GameFormat {
    match format {
        "ZSO" => GameFormat::ZSO,
        "UL" => GameFormat::UL,
        _ => GameFormat::ISO,
    }
}

pub(crate) fn step(plan: &core::Plan, step: &Step) -> PlanStep {
    let source = |index: usize| plan.sources.get(index).map(|s| display_path(&s.path));
    let base = |op, path: &RelPath| PlanStep {
        op,
        path: path.to_string(),
        to: None,
        bytes: None,
        removal: None,
        source: None,
    };
    match step {
        Step::CreateDir { path } => base(StepOp::CreateDir, path),
        Step::RemoveDir { path } => base(StepOp::RemoveDir, path),
        Step::WriteFile {
            path,
            source: s,
            len,
            ..
        } => PlanStep {
            bytes: Some(*len),
            source: source(*s),
            ..base(StepOp::WriteFile, path)
        },
        Step::WriteZso {
            path,
            source: s,
            max_len,
        } => PlanStep {
            bytes: Some(*max_len),
            source: source(*s),
            ..base(StepOp::WriteZso, path)
        },
        Step::WriteVmc { path, size_mb, .. } => PlanStep {
            bytes: Some(u64::from(*size_mb) << 20),
            ..base(StepOp::WriteVmc, path)
        },
        Step::PutFile { path, content, .. } => PlanStep {
            bytes: Some(content.0.len() as u64),
            ..base(StepOp::PutFile, path)
        },
        Step::CopyFile { from, to } => PlanStep {
            to: Some(to.to_string()),
            ..base(StepOp::CopyFile, from)
        },
        Step::Rename { from, to } => PlanStep {
            to: Some(to.to_string()),
            ..base(StepOp::Rename, from)
        },
        Step::Remove { path, len, what } => PlanStep {
            bytes: Some(*len),
            removal: Some(match what {
                Removal::UserData => RemovalKind::UserData,
                Removal::Cache => RemovalKind::Cache,
                Removal::Ours | Removal::OursZso { .. } | Removal::Copied { .. } => {
                    RemovalKind::Ours
                }
                Removal::Backup => RemovalKind::Backup,
            }),
            ..base(StepOp::Remove, path)
        },
        Step::RemoveSource { source: s } => PlanStep {
            op: StepOp::RemoveSource,
            path: source(*s).unwrap_or_default(),
            to: None,
            bytes: plan
                .sources
                .get(*s)
                .map(|src| src.files.iter().map(|f| f.len).sum()),
            removal: Some(RemovalKind::UserData),
            source: None,
        },
        Step::DropJournal { id } => PlanStep {
            op: StepOp::DropJournal,
            path: id.clone(),
            to: None,
            bytes: None,
            removal: None,
            source: None,
        },
    }
}

impl PlanView {
    pub fn new(id: String, root: &Path, plan: &core::Plan) -> Self {
        Self {
            id,
            kind: kind(plan.kind),
            title: plan.title.clone(),
            title_phrase: plan.title_phrase.as_ref().map(Phrase::from),
            root: display_path(root),
            write_bytes: plan.write_bytes(),
            delete_bytes: plan.delete_bytes(),
            free_bytes: plan.free_bytes,
            executable: plan.is_executable(),
            deletes_user_data: plan.deletes_user_data(),
            transactions: plan
                .transactions
                .iter()
                .map(|tx| PlanTransaction {
                    title: tx.title.clone(),
                    title_phrase: tx.title_phrase.as_ref().map(Phrase::from),
                    game: tx.game.as_ref().map(|g| PlanGame {
                        title: g.title.clone(),
                        startup: g.startup.clone(),
                        media: g.media.into(),
                        format: format(&g.format),
                    }),
                    steps: tx.steps.iter().map(|s| step(plan, s)).collect(),
                    commit: u32::try_from(tx.commit).unwrap_or(u32::MAX),
                    notices: tx.notices.iter().map(PlanNotice::from).collect(),
                    cfg_changes: tx
                        .cfg_changes
                        .iter()
                        .map(|c| CfgKeyChange {
                            key: c.key.clone(),
                            before: c.before.clone(),
                            after: c.after.clone(),
                        })
                        .collect(),
                })
                .collect(),
            skipped: plan
                .skipped
                .iter()
                .map(|s| PlanSkipped {
                    path: display_path(&s.path),
                    reason: (&s.reason).into(),
                })
                .collect(),
            notices: plan.notices.iter().map(PlanNotice::from).collect(),
        }
    }
}

pub struct StoredPlan {
    pub plan: core::Plan,
    pub root: PathBuf,
    /// The device's mode the plan was made for; `None`: its place's
    /// (ADR-0047).
    pub mode: Option<yaoplm_device::DeviceMode>,
    created: Instant,
}

/// Pictures a search found for the games of an import plan (ART-8), kept
/// for `PlanImport.covers` like a plan: an hour at most.
#[derive(Debug, Clone)]
pub struct CoverSet {
    pub root: PathBuf,
    pub items: Vec<ArtItem>,
    /// Games with nothing found, and why: by game ID.
    pub missing: Vec<(String, core::Skipped)>,
    created: Instant,
}

/// Plans waiting for confirmation. Clones share the store, so a job can
/// add the plan it produced.
#[derive(Default, Clone)]
pub struct PlanStore {
    plans: Arc<Mutex<HashMap<String, StoredPlan>>>,
    covers: Arc<Mutex<HashMap<String, CoverSet>>>,
}

impl std::fmt::Debug for PlanStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlanStore")
            .field("plans", &self.lock().len())
            .finish()
    }
}

impl PlanStore {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, StoredPlan>> {
        self.plans.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Keeps `plan` and returns its view.
    pub fn insert(&self, root: PathBuf, plan: core::Plan) -> PlanView {
        self.insert_in(root, None, plan)
    }

    /// Keeps `plan`, made for the device in `mode`.
    pub fn insert_in(
        &self,
        root: PathBuf,
        mode: Option<yaoplm_device::DeviceMode>,
        plan: core::Plan,
    ) -> PlanView {
        let id = yaoplm_core::journal::new_id();
        let view = PlanView::new(id.clone(), &root, &plan);
        let mut plans = self.lock();
        plans.retain(|_, p| p.created.elapsed() < PLAN_TTL);
        while plans.len() >= MAX_PLANS {
            let Some(oldest) = plans
                .iter()
                .min_by_key(|(_, p)| p.created)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            plans.remove(&oldest);
        }
        plans.insert(
            id,
            StoredPlan {
                plan,
                root,
                mode,
                created: Instant::now(),
            },
        );
        view
    }

    /// Takes a plan out for execution, after `check` approved it. A plan
    /// that `check` refuses stays.
    pub fn take(
        &self,
        id: &str,
        check: impl FnOnce(&StoredPlan) -> Result<(), ApiError>,
    ) -> Result<StoredPlan, ApiError> {
        let mut plans = self.lock();
        let stored = plans
            .get(id)
            .filter(|p| p.created.elapsed() < PLAN_TTL)
            .ok_or_else(|| expired(id))?;
        check(stored)?;
        plans.remove(id).ok_or_else(|| expired(id))
    }

    pub fn discard(&self, id: &str) -> bool {
        self.lock().remove(id).is_some()
    }

    /// A copy of a plan that is still waiting, and its root.
    pub fn peek(&self, id: &str) -> Option<(core::Plan, PathBuf)> {
        self.lock()
            .get(id)
            .filter(|p| p.created.elapsed() < PLAN_TTL)
            .map(|p| (p.plan.clone(), p.root.clone()))
    }

    /// Keeps a cover set and returns its id.
    pub fn insert_covers(
        &self,
        root: PathBuf,
        items: Vec<ArtItem>,
        missing: Vec<(String, core::Skipped)>,
    ) -> String {
        let id = yaoplm_core::journal::new_id();
        let mut covers = self.covers.lock().unwrap_or_else(PoisonError::into_inner);
        covers.retain(|_, c| c.created.elapsed() < PLAN_TTL);
        while covers.len() >= MAX_PLANS {
            let Some(oldest) = covers
                .iter()
                .min_by_key(|(_, c)| c.created)
                .map(|(id, _)| id.clone())
            else {
                break;
            };
            covers.remove(&oldest);
        }
        covers.insert(
            id.clone(),
            CoverSet {
                root,
                items,
                missing,
                created: Instant::now(),
            },
        );
        id
    }

    /// A cover set, which stays for the next re-plan.
    pub fn covers(&self, id: &str) -> Option<CoverSet> {
        self.covers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .filter(|c| c.created.elapsed() < PLAN_TTL)
            .cloned()
    }
}

fn expired(id: &str) -> ApiError {
    ApiError::phrased(
        ErrorCode::NotFound,
        &yaoplm_core::Phrase::error("planExpired").with("id", id),
    )
}
