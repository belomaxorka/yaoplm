use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ApiError;
use crate::devices::DeviceInfo;
use crate::jobs::{JobInfo, JobKind};
use crate::library::{ImageDetails, LoaderProfile, Media};
use crate::plans::PlanView;

/// `ID.Title.iso` (ADR-0006, the default) or `Title.iso`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum NameStyle {
    #[default]
    IdTitle,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Transfer {
    #[default]
    Copy,
    /// Delete each source once its game is on the device (IMP-11).
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    /// `ul` only where the file system needs it (FAT32, over 4 GiB).
    #[default]
    Auto,
    /// Always `ul` chunks.
    Ul,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum RecoveryAction {
    /// Undo what was not committed, finish what was.
    #[default]
    Rollback,
    /// Redo what was not committed, then the rest.
    Resume,
}

/// A per-source choice from the plan screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ImportOverride {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts-export", ts(optional))]
    pub media: Option<Media>,
}

/// A game on the device, as `ScanLibrary` reported it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GameRef {
    /// An image in `CD/` or `DVD/`: its path, absolute or relative to the root.
    Image { path: String },
    /// A `ul.cfg` record; the ID and title must still match.
    Ul {
        index: u32,
        startup: String,
        name: String,
    },
}

/// An operation requested by a front end.
///
/// Serialized as a JSON object tagged by `type`, e.g. `{"type":"Ping"}`.
/// Writes are always two requests: one that returns a [`PlanView`], and
/// `ExecutePlan` with its id once the user confirmed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum Request {
    /// Liveness check; answered with [`Response::Pong`].
    Ping,
    /// Application name and version.
    Version,
    /// Mounted volumes with the storage doctor's verdict (DEV-1, DEV-2).
    /// The server lists its library roots only.
    ListDevices {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
    },
    /// One volume or folder in detail (DEV-2).
    InspectDevice {
        root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// OPL is started from this device by FreeMcBoot or old uLaunchELF.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        boot_from_device: Option<bool>,
    },
    /// Lists a device root (or its `usb_prefix` folder) as OPL would. A
    /// job: answered with [`Response::Job`]; the library is its result.
    ScanLibrary {
        root: String,
        /// Defaults to OPL 1.2.0 beta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Also read images whose ID OPL takes from the name or
        /// `games.bin`, to catch mismatches. Defaults to true.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        verify: Option<bool>,
    },
    /// Details of one ISO or ZSO file. Answered with [`Response::ImageInfo`].
    ImageInfo {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
    },
    /// Plans creating the missing OPL folders (LIB-3).
    PlanInit {
        root: String,
    },
    /// Plans copying ISO, ZSO and BIN/CUE files or folders to a device
    /// (IMP-1 to IMP-11).
    PlanImport {
        root: String,
        sources: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        name_style: Option<NameStyle>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        transfer: Option<Transfer>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        layout: Option<Layout>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        overrides: Option<Vec<ImportOverride>>,
    },
    /// Plans a rename (GAME-1). A new `startup` also moves CFG, art and
    /// cheats.
    RenameGame {
        root: String,
        game: GameRef,
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        startup: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        name_style: Option<NameStyle>,
    },
    /// Plans deleting a game (GAME-2); `related` also deletes its CFG, art
    /// and cheats when no other game uses the ID.
    DeleteGame {
        root: String,
        game: GameRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        related: Option<bool>,
    },
    /// Plans moving a game between `CD/` and `DVD/` (GAME-3).
    MoveGame {
        root: String,
        game: GameRef,
        media: Media,
    },
    /// Plans finishing or undoing operations a crash left half done.
    PlanRecovery {
        root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        action: Option<RecoveryAction>,
    },
    /// Runs a plan. A plan that deletes the user's files also needs
    /// `confirmDelete: true`. Answered with [`Response::Job`].
    ExecutePlan {
        plan: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        confirm_delete: Option<bool>,
    },
    /// Forgets a plan the user did not confirm.
    DiscardPlan {
        plan: String,
    },
    /// A job's state, with its result once finished.
    GetJob {
        job: JobId,
    },
    ListJobs,
    CancelJob {
        job: JobId,
    },
}

/// The successful answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum Response {
    Pong,
    Version {
        name: String,
        version: String,
    },
    /// The request was carried out and has nothing to report.
    Ack,
    Devices {
        devices: Vec<DeviceInfo>,
    },
    Device(Box<DeviceInfo>),
    ImageInfo(Box<ImageDetails>),
    Plan(Box<PlanView>),
    /// A job was queued; follow it with [`JobEvent`]s or `GetJob`.
    Job {
        job: JobId,
    },
    JobState(Box<JobInfo>),
    Jobs {
        jobs: Vec<JobInfo>,
    },
}

/// Identifies a long-running job for its whole lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct JobId(pub u64);

/// Progress of a long-running job, streamed to subscribers.
///
/// Every job emits `Queued`, `Started`, any number of `Progress` events,
/// then exactly one of `Finished`, `Failed` or `Cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum JobEvent {
    Queued {
        job: JobId,
        kind: JobKind,
        title: String,
    },
    Started {
        job: JobId,
    },
    /// Work done so far: bytes for plans, entries for scans. `total` is
    /// `None` while the size is still unknown; `rate` is bytes per second.
    Progress {
        job: JobId,
        done: u64,
        total: Option<u64>,
        rate: Option<u64>,
        /// What is being worked on, e.g. the game being copied.
        item: Option<String>,
    },
    Finished {
        job: JobId,
    },
    Failed {
        job: JobId,
        error: ApiError,
    },
    Cancelled {
        job: JobId,
    },
}
