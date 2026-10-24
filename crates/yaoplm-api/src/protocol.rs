use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-export")]
use ts_rs::TS;

use crate::ApiError;
use crate::art::{ArtCacheInfo, ArtInput, ArtScope, ArtSourceSetting, ArtType, Attribution};
use crate::devices::{DeviceInfo, DeviceMode, ModeOverride, ProfileOverride};
use crate::dir::DirListing;
use crate::jobs::{JobInfo, JobKind, JobLogEntry};
use crate::library::{ImageDetails, LoaderProfile, Media};
use crate::loaders::LoaderCatalog;
use crate::phrase::Phrase;
use crate::plans::PlanView;
use crate::settings::{CfgPair, CfgView, GameSettings, NewVmc, VmcCard};
use crate::verify::RedumpInfo;

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

/// What `ConvertGames` turns games into (GAME-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ConvertTarget {
    Iso,
    Zso,
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
        /// A profile forced for every device in this request (the CLI's
        /// `--profile`); absent: auto, each device by its files (LIB-1,
        /// ADR-0040). The interface never sends it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Profiles chosen per device (`ActiveProfile::key`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        overrides: Option<Vec<ProfileOverride>>,
        /// Modes chosen per device, by the same key (ADR-0043); every
        /// other device gets its place's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode_overrides: Option<Vec<ModeOverride>>,
    },
    /// One volume or folder in detail (DEV-2).
    InspectDevice {
        root: String,
        /// As in `ListDevices`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        overrides: Option<Vec<ProfileOverride>>,
        /// OPL is started from this device by FreeMcBoot or old uLaunchELF.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        boot_from_device: Option<bool>,
        /// A mode forced for this request (the CLI's `--mode`); absent: an
        /// override, else the device's place (ADR-0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode: Option<DeviceMode>,
        /// As in `ListDevices`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode_overrides: Option<Vec<ModeOverride>>,
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
        /// How OPL reads the device (`DeviceInfo.mode`); absent: by its
        /// place. Over SMB the fragment rule does not apply (ADR-0043).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode: Option<DeviceMode>,
    },
    /// Details of one ISO or ZSO file. Answered with [`Response::ImageInfo`].
    ImageInfo {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
    },
    /// Lists a folder so the web UI can pick import sources (IMP-1). The
    /// server only lists its library roots and what is inside them; without
    /// `path` the roots (or, on the desktop, the volumes) are listed.
    /// Answered with [`Response::Dir`].
    ListDir {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        path: Option<String>,
    },
    /// Plans creating the missing OPL folders (LIB-3). With `folder`, a
    /// folder of that name inside `root` with the OPL folders in it: a
    /// library of its own for a whole system disk (ADR-0043).
    PlanInit {
        root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        folder: Option<String>,
    },
    /// Plans copying ISO, ZSO, BIN/CUE and CHD images, also from zip and 7z
    /// archives, or folders of them, to a device (IMP-1 to IMP-11, IMP-1b).
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
        /// Store images as ZSO (OPL 1.2.0 beta; never for `ul`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        compress: Option<bool>,
        /// Hash the images while they are written and check them against
        /// the stored Redump DAT (GAME-5); the verdicts land in the job log
        /// and the hash cache. Off when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        verify: Option<bool>,
        /// Title the games the bundled database knows by its name there;
        /// the others keep their file name's title. Off when absent: only
        /// games whose file names give no title are named by the database.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        database_titles: Option<bool>,
        /// A cover set from `FetchArt` with `import` (ART-8): every game
        /// gets its cover written right after it, in the same plan.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        covers: Option<String>,
        /// As in `ScanLibrary`: SMB's limits or BDM's fragment rule.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode: Option<DeviceMode>,
    },
    /// Plans converting games between ISO, ZSO and `ul` (GAME-4). The old
    /// form is deleted after the new one is complete, so executing needs
    /// `confirmDelete`.
    ConvertGames {
        root: String,
        games: Vec<GameRef>,
        target: ConvertTarget,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// How images made from `ul` chunks are named.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        name_style: Option<NameStyle>,
        /// As in `ScanLibrary`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode: Option<DeviceMode>,
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
        /// As in `ScanLibrary`: over SMB a renamed `ul` game needs OPL
        /// restarted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        mode: Option<DeviceMode>,
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
    /// Finds covers for the games on a device (ART-1, ART-4): a job whose
    /// result is a plan to write them (`JobResult::Plan`). Network access
    /// happens only here and only for the games in scope. With `import`,
    /// the games are those an import plan adds (ART-8) and the result is a
    /// cover set for `PlanImport.covers` (`JobResult::Covers`).
    FetchArt {
        root: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Defaults to `missing`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        scope: Option<ArtScope>,
        /// Only these game IDs; every game when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        games: Option<Vec<String>>,
        /// Per-source switches and mirrors.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        sources: Option<Vec<ArtSourceSetting>>,
        /// One picture type by its suffix; `kinds` takes several.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        kind: Option<String>,
        /// The picture types to look for (ART-2); COV when neither this nor
        /// `kind` is given.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        kinds: Option<Vec<ArtType>>,
        /// The id of an import plan: look for the pictures (`kinds`) of the
        /// games it adds, `missing` scope (ART-8). Ends with a set for
        /// `PlanImport.covers`, not a plan.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        import: Option<String>,
    },
    /// Every picture of one type the sources have for one game, to pick
    /// from (ART-2, SPEC §5.4.1): a job whose result is `ArtVariants`.
    /// The chosen one goes back through `SetArt` as bytes.
    FindArt {
        root: String,
        startup: String,
        kind: ArtType,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        sources: Option<Vec<ArtSourceSetting>>,
    },
    /// Pictures from picture sets the user has (ART-5): OPL Manager's art
    /// backups (zip or 7z), folders of pictures, `art.tar`. A job that ends
    /// with a plan writing the pictures of the games on the device; the
    /// rest of a set is left out.
    ImportArt {
        root: String,
        /// Folders, archives or picture files.
        sources: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Every type when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        kinds: Option<Vec<ArtType>>,
        /// Replace pictures the device has; they move to the backup folder.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        overwrite: Option<bool>,
    },
    /// Plans writing a cover the user supplied (ART-3): a file on the host
    /// or image bytes, transcoded like a downloaded one.
    SetArt {
        root: String,
        startup: String,
        input: ArtInput,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        kind: Option<String>,
        /// Replace an existing cover (moved to the backup folder first).
        /// Defaults to true.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        overwrite: Option<bool>,
    },
    /// A game's settings: what its CFG says (CFG-2). Answered with
    /// [`Response::Cfg`].
    ReadCfg {
        root: String,
        game: GameRef,
    },
    /// Plans changing a game's CFG the way OPL's game menu saves it
    /// (CFG-2), with a backup first (CFG-5): `settings` as they should be,
    /// then `set` and `unset` keys as given. `cards` are blank VMCs
    /// created in the same transaction (VMC-1).
    WriteCfg {
        root: String,
        game: GameRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        settings: Option<GameSettings>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        set: Option<Vec<CfgPair>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        unset: Option<Vec<String>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        cards: Option<Vec<NewVmc>>,
    },
    /// The memory cards in `VMC/` and the games that use them (VMC-1).
    /// Answered with [`Response::Vmcs`].
    ListVmc {
        root: String,
    },
    /// Plans blank, formatted memory cards (VMC-1).
    CreateVmc {
        root: String,
        cards: Vec<NewVmc>,
    },
    /// Plans deleting a cover, with a copy in the backup folder.
    DeleteArt {
        root: String,
        startup: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        kind: Option<String>,
    },
    /// Hashes games on a device and matches them against the stored Redump
    /// DAT (GAME-5): a job whose result is a `VerificationReport`. Reads
    /// only; unchanged images come from the hash cache.
    VerifyGames {
        root: String,
        /// Only these games; every game when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        games: Option<Vec<GameRef>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        profile: Option<LoaderProfile>,
        /// Also compute MD5 (CRC-32 and SHA-1 always).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        md5: Option<bool>,
        /// Read every image even when its hashes are cached: finds damage
        /// that left a file's size and time alone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        fresh: Option<bool>,
    },
    /// The stored Redump DAT, if any.
    RedumpStatus,
    /// A job that downloads the Redump DAT (at most once a week, META-2) or,
    /// with `file`, imports a `.dat` or Redump `.zip` the user has.
    UpdateRedump {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(feature = "ts-export", ts(optional))]
        file: Option<String>,
    },
    /// Credits, licences and versions of the bundled data and the cover
    /// sources, for the About screen.
    Attribution,
    /// Where the art cache is and how big it is.
    ArtCache,
    /// Deletes the art cache; nothing on any device changes.
    ClearArtCache,
    /// The loader builds YAOPLM installs, with their channels and presets
    /// (LDR-1).
    LoaderCatalog,
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
    Dir(Box<DirListing>),
    Plan(Box<PlanView>),
    /// A job was queued; follow it with [`JobEvent`]s or `GetJob`.
    Job {
        job: JobId,
    },
    JobState(Box<JobInfo>),
    Jobs {
        jobs: Vec<JobInfo>,
    },
    Attribution(Box<Attribution>),
    ArtCache(ArtCacheInfo),
    /// The stored Redump DAT; `None` before the first download or import.
    Redump {
        dat: Option<Box<RedumpInfo>>,
    },
    Cfg(Box<CfgView>),
    Vmcs {
        cards: Vec<VmcCard>,
    },
    LoaderCatalog(Box<LoaderCatalog>),
}

/// Identifies a long-running job for its whole lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
pub struct JobId(pub u64);

/// Progress of a long-running job, streamed to subscribers.
///
/// Every job emits `Queued`, `Started`, any number of `Progress` and `Log`
/// events, then exactly one of `Finished`, `Failed` or `Cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum JobEvent {
    Queued {
        job: JobId,
        kind: JobKind,
        title: String,
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_phrase: Option<Phrase>,
        /// The device the job works on, when it has one.
        root: Option<String>,
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
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_phrase: Option<Phrase>,
    },
    /// A line of a job's log: a transaction or step of a plan, a game of a
    /// cover search or dump check. `GetJob` returns the whole log, so a
    /// client that reconnects misses nothing.
    Log {
        job: JobId,
        entry: JobLogEntry,
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
