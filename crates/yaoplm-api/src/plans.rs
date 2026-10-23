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
                Removal::Ours | Removal::OursZso { .. } => RemovalKind::Ours,
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
    created: Instant,
}

/// Plans waiting for confirmation. Clones share the store, so a job can
/// add the plan it produced.
#[derive(Default, Clone)]
pub struct PlanStore {
    plans: Arc<Mutex<HashMap<String, StoredPlan>>>,
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
}

fn expired(id: &str) -> ApiError {
    ApiError::phrased(
        ErrorCode::NotFound,
        &yaoplm_core::Phrase::error("planExpired").with("id", id),
    )
}
