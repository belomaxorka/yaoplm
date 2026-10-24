//! Long-running work as jobs (IMP-9): every job gets a [`JobId`], streams
//! [`JobEvent`]s and can be cancelled.
//!
//! Jobs that write to a device run one at a time on a single worker, in
//! the order they were queued: two copies to one stick at once would
//! interleave their clusters and fragment both games (OPL loads at most 64
//! fragments per game; its README asks to copy games one by one). Read-only
//! jobs (scans) run on their own threads.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
#[cfg(feature = "ts-export")]
use ts_rs::TS;
use yaoplm_device::CancelToken;

use crate::art::{ArtLogState, ArtVariants, CoverSearch};
use crate::events::EventBus;
use crate::library::Library;
use crate::phrase::Phrase;
use crate::plans::{PlanNotice, PlanView, StepOp};
use crate::verify::{
    GameVerification, RedumpResult, UnknownReason, VerificationReport, VerifyStatus,
};
use crate::{ApiError, ErrorCode, JobEvent, JobId};

/// Finished jobs kept for `GetJob` / `ListJobs`.
const KEEP_FINISHED: usize = 100;
/// At most this many progress events per second and job.
const EVENT_INTERVAL: Duration = Duration::from_millis(100);
/// Oldest log entries are dropped beyond this many per job.
const LOG_LIMIT: usize = 2000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum JobKind {
    ScanLibrary,
    ExecutePlan,
    /// Finds covers and ends with a plan to write them (ART-1, ART-4).
    FetchArt,
    /// Hashes games and matches them against Redump (GAME-5).
    VerifyGames,
    /// Downloads or imports the Redump DAT (META-2).
    UpdateRedump,
    /// Reads picture sets and ends with a plan to write them (ART-5).
    ImportArt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Queued,
    Running,
    Finished,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_final(self) -> bool {
        matches!(self, Self::Finished | Self::Failed | Self::Cancelled)
    }
}

/// How one transaction of an executed plan ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TxOutcome {
    Done,
    RolledBack,
    NotStarted,
    /// Half done; `journal` names what recovery will pick up.
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ExecutionReport {
    /// One per transaction of the plan, in order.
    pub outcomes: Vec<TxOutcome>,
    pub cancelled: bool,
    pub error: Option<String>,
    /// `error` for front ends that translate it.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_phrase: Option<Phrase>,
    /// A journal left on the device: run `PlanRecovery`.
    pub journal: Option<String>,
    /// Verdicts for the games the plan hashed while writing them (GAME-5,
    /// `PlanImport` with `verify`).
    #[serde(default)]
    pub checks: Vec<GameVerification>,
    /// What the writes found once done, such as a memory card the volume
    /// could only give in several fragments (VMC-1).
    #[serde(default)]
    pub notices: Vec<PlanNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(tag = "type")]
pub enum JobResult {
    Library(Box<Library>),
    Execution(ExecutionReport),
    /// A plan to confirm with `ExecutePlan` (`FetchArt`).
    Plan(Box<PlanView>),
    /// Covers for the games of an import plan (`FetchArt` with `import`).
    Covers(CoverSearch),
    /// Pictures to pick from for one game (`FindArt`).
    ArtVariants(Box<ArtVariants>),
    Verification(Box<VerificationReport>),
    Redump(Box<RedumpResult>),
}

/// The state of a transaction in the live log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum TxLogState {
    Started,
    Done,
    RolledBack,
    Interrupted,
}

/// The state of a step in the live log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum StepLogState {
    Started,
    Done,
    Failed,
    /// Rolled back after a failure or a cancel.
    Undone,
}

/// One line of a job's live log: plans log transactions and steps, cover
/// searches and dump checks one line per game (the idea comes from
/// OrbitPS2 Manager's per-job log, research/10).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum JobLogEntry {
    Transaction {
        /// Milliseconds since the Unix epoch.
        at: u64,
        index: u32,
        title: String,
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title_phrase: Option<Phrase>,
        state: TxLogState,
    },
    Step {
        at: u64,
        transaction: u32,
        index: u32,
        op: StepOp,
        /// Device path relative to the root, or a host path for sources.
        path: String,
        to: Option<String>,
        state: StepLogState,
        error: Option<String>,
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_phrase: Option<Phrase>,
    },
    /// One game of a `FetchArt` job.
    Art {
        at: u64,
        startup: String,
        title: String,
        /// `COV`.
        art: String,
        state: ArtLogState,
        /// The source that had it.
        source: Option<String>,
        /// Where it came from, or why it is missing.
        detail: Option<String>,
        /// Why it is missing, for front ends that translate it.
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail_phrase: Option<Phrase>,
    },
    /// One game of a `VerifyGames` job.
    Verify {
        at: u64,
        startup: String,
        title: String,
        status: VerifyStatus,
        reason: Option<UnknownReason>,
        /// The Redump name of the matching dump.
        redump: Option<String>,
        /// The hashes came from the cache.
        cached: bool,
        error: Option<String>,
        #[cfg_attr(feature = "ts-export", ts(optional))]
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_phrase: Option<Phrase>,
    },
}

/// A job as front ends see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-export", derive(TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct JobInfo {
    pub id: JobId,
    pub kind: JobKind,
    /// English; `title_phrase` says it in the front end's language.
    pub title: String,
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_phrase: Option<Phrase>,
    /// The device the job works on, when it has one.
    pub root: Option<String>,
    pub status: JobStatus,
    /// Bytes for plans and verification, items for scans.
    pub done: u64,
    pub total: Option<u64>,
    /// Bytes per second since the job started (plans and dump checks).
    pub rate: Option<u64>,
    /// What is being worked on, e.g. the game being copied.
    pub item: Option<String>,
    /// `item` as a phrase, when it is a text rather than a name.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_phrase: Option<Phrase>,
    /// Set once the job finished; `ListJobs` leaves it out.
    #[cfg_attr(feature = "ts-export", ts(optional))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JobResult>,
    pub error: Option<ApiError>,
    /// The job's log so far (see [`JobLogEntry`]); `ListJobs` leaves it out.
    #[serde(default)]
    pub log: Vec<JobLogEntry>,
}

/// Which queue a job runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Queue {
    Read,
    Write,
}

struct Record {
    info: JobInfo,
    cancel: CancelToken,
}

struct Inner {
    next: AtomicU64,
    jobs: Mutex<BTreeMap<u64, Record>>,
    changed: Condvar,
    events: EventBus,
    writer: Mutex<Option<mpsc::Sender<Task>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs jobs and keeps their state.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for JobManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobManager")
            .field("jobs", &lock(&self.inner.jobs).len())
            .finish()
    }
}

/// Handed to a running job.
pub struct JobContext {
    id: JobId,
    inner: Arc<Inner>,
    cancel: CancelToken,
    started: Instant,
    last_event: Mutex<Option<Instant>>,
    rate: bool,
}

impl JobContext {
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Records progress; events go out at most every 100 ms, and always
    /// when `done == total`.
    pub fn progress(&self, done: u64, total: Option<u64>, item: Option<&str>) {
        self.progress_with(done, total, item, None);
    }

    /// [`JobContext::progress`] with the item as a phrase too.
    pub fn progress_with(
        &self,
        done: u64,
        total: Option<u64>,
        item: Option<&str>,
        item_phrase: Option<&yaoplm_core::Phrase>,
    ) {
        let item_phrase = item_phrase.map(Phrase::from);
        let rate = self.rate.then(|| {
            let secs = self.started.elapsed().as_secs_f64();
            (secs > 0.5).then(|| (done as f64 / secs) as u64)
        });
        let rate = rate.flatten();
        {
            let mut jobs = lock(&self.inner.jobs);
            if let Some(record) = jobs.get_mut(&self.id.0) {
                record.info.done = done;
                record.info.total = total;
                record.info.rate = rate;
                if let Some(item) = item {
                    record.info.item = Some(item.to_owned());
                    record.info.item_phrase.clone_from(&item_phrase);
                }
            }
        }
        let now = Instant::now();
        let mut last = lock(&self.last_event);
        let due =
            last.is_none_or(|t| now.duration_since(t) >= EVENT_INTERVAL) || total == Some(done);
        if due {
            *last = Some(now);
            drop(last);
            self.inner.events.publish(&JobEvent::Progress {
                job: self.id,
                done,
                total,
                rate,
                item: item.map(str::to_owned),
                item_phrase,
            });
        }
    }

    /// Appends to the job's log and publishes the entry at once.
    pub fn log(&self, entry: JobLogEntry) {
        {
            let mut jobs = lock(&self.inner.jobs);
            if let Some(record) = jobs.get_mut(&self.id.0) {
                if record.info.log.len() >= LOG_LIMIT {
                    record.info.log.remove(0);
                }
                record.info.log.push(entry.clone());
            }
        }
        self.inner.events.publish(&JobEvent::Log {
            job: self.id,
            entry,
        });
    }
}

/// `NotFound` for a job this server does not know (any more).
pub(crate) fn job_gone(id: JobId) -> ApiError {
    ApiError::phrased(
        ErrorCode::NotFound,
        &yaoplm_core::Phrase::error("jobGone").with("id", id.0),
    )
}

/// Milliseconds since the Unix epoch, for log entries.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

type Work = Box<dyn FnOnce(&JobContext) -> Result<JobResult, ApiError> + Send>;
type Task = Box<dyn FnOnce() + Send>;

impl JobManager {
    pub fn new(events: EventBus) -> Self {
        Self {
            inner: Arc::new(Inner {
                next: AtomicU64::new(1),
                jobs: Mutex::new(BTreeMap::new()),
                changed: Condvar::new(),
                events,
                writer: Mutex::new(None),
            }),
        }
    }

    /// Queues `work`; returns at once.
    pub fn spawn(
        &self,
        kind: JobKind,
        title: &yaoplm_core::Phrase,
        root: Option<String>,
        queue: Queue,
        work: impl FnOnce(&JobContext) -> Result<JobResult, ApiError> + Send + 'static,
    ) -> JobId {
        let id = JobId(self.inner.next.fetch_add(1, Ordering::Relaxed));
        let cancel = CancelToken::new();
        let info = JobInfo {
            id,
            kind,
            title: title.english(),
            title_phrase: Some(Phrase::from(title)),
            root,
            status: JobStatus::Queued,
            done: 0,
            total: None,
            rate: None,
            item: None,
            item_phrase: None,
            result: None,
            error: None,
            log: Vec::new(),
        };
        {
            let mut jobs = lock(&self.inner.jobs);
            jobs.insert(
                id.0,
                Record {
                    info: info.clone(),
                    cancel: cancel.clone(),
                },
            );
            prune(&mut jobs);
        }
        self.inner.events.publish(&JobEvent::Queued {
            job: id,
            kind,
            title: info.title,
            title_phrase: info.title_phrase,
            root: info.root,
        });
        let inner = Arc::clone(&self.inner);
        let work: Work = Box::new(work);
        let run = move || run_job(&inner, id, cancel, kind, work);
        match queue {
            Queue::Read => {
                thread::spawn(run);
            }
            Queue::Write => self.write_queue().send_or_run(Box::new(run)),
        }
        id
    }

    fn write_queue(&self) -> WriteQueue<'_> {
        WriteQueue(&self.inner)
    }

    /// Asks a job to stop. Writes stop at the next block and undo the game
    /// in progress; a queued job never starts.
    pub fn cancel(&self, id: JobId) -> Result<(), ApiError> {
        let jobs = lock(&self.inner.jobs);
        let record = jobs.get(&id.0).ok_or_else(|| job_gone(id))?;
        record.cancel.cancel();
        Ok(())
    }

    pub fn get(&self, id: JobId) -> Option<JobInfo> {
        lock(&self.inner.jobs).get(&id.0).map(|r| r.info.clone())
    }

    /// All known jobs, newest last, without their results and logs.
    pub fn list(&self) -> Vec<JobInfo> {
        lock(&self.inner.jobs)
            .values()
            .map(|r| JobInfo {
                result: None,
                log: Vec::new(),
                ..r.info.clone()
            })
            .collect()
    }

    /// Blocks until the job has finished (CLI, tests).
    pub fn wait(&self, id: JobId) -> Option<JobInfo> {
        let mut jobs = lock(&self.inner.jobs);
        loop {
            let record = jobs.get(&id.0)?;
            if record.info.status.is_final() {
                return Some(record.info.clone());
            }
            jobs = self
                .inner
                .changed
                .wait(jobs)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

struct WriteQueue<'a>(&'a Inner);

impl WriteQueue<'_> {
    fn send_or_run(&self, job: Task) {
        let mut writer = lock(&self.0.writer);
        let sender = writer.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Task>();
            thread::spawn(move || {
                for job in rx {
                    job();
                }
            });
            tx
        });
        if let Err(mpsc::SendError(job)) = sender.send(job) {
            // The worker is gone (it panicked): start a fresh one.
            *writer = None;
            drop(writer);
            thread::spawn(job);
        }
    }
}

fn prune(jobs: &mut BTreeMap<u64, Record>) {
    let finished: Vec<u64> = jobs
        .iter()
        .filter(|(_, r)| r.info.status.is_final())
        .map(|(id, _)| *id)
        .collect();
    if finished.len() > KEEP_FINISHED {
        for id in &finished[..finished.len() - KEEP_FINISHED] {
            jobs.remove(id);
        }
    }
}

fn update(inner: &Inner, id: JobId, change: impl FnOnce(&mut JobInfo)) {
    if let Some(record) = lock(&inner.jobs).get_mut(&id.0) {
        change(&mut record.info);
    }
}

/// Records the final state, publishes the final event, then wakes
/// `wait()`ers: whoever waited has seen every event of the job.
fn finish(inner: &Inner, id: JobId, change: impl FnOnce(&mut JobInfo), event: &JobEvent) {
    update(inner, id, change);
    inner.events.publish(event);
    inner.changed.notify_all();
}

fn run_job(inner: &Arc<Inner>, id: JobId, cancel: CancelToken, kind: JobKind, work: Work) {
    if cancel.is_cancelled() {
        finish(
            inner,
            id,
            |info| info.status = JobStatus::Cancelled,
            &JobEvent::Cancelled { job: id },
        );
        return;
    }
    update(inner, id, |info| info.status = JobStatus::Running);
    inner.events.publish(&JobEvent::Started { job: id });
    let context = JobContext {
        id,
        inner: Arc::clone(inner),
        cancel,
        started: Instant::now(),
        last_event: Mutex::new(None),
        rate: matches!(kind, JobKind::ExecutePlan | JobKind::VerifyGames),
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&context)))
        .unwrap_or_else(|_| {
            Err(ApiError::phrased(
                ErrorCode::Internal,
                &yaoplm_core::Phrase::error("jobPanicked"),
            ))
        });
    match result {
        Ok(result) => {
            let cancelled = matches!(&result, JobResult::Execution(r) if r.cancelled);
            let (status, event) = if cancelled {
                (JobStatus::Cancelled, JobEvent::Cancelled { job: id })
            } else {
                (JobStatus::Finished, JobEvent::Finished { job: id })
            };
            finish(
                inner,
                id,
                |info| {
                    info.status = status;
                    info.result = Some(result);
                },
                &event,
            );
        }
        Err(error) if error.code == ErrorCode::Cancelled => finish(
            inner,
            id,
            |info| {
                info.status = JobStatus::Cancelled;
                info.error = Some(error);
            },
            &JobEvent::Cancelled { job: id },
        ),
        Err(error) => {
            let event = JobEvent::Failed {
                job: id,
                error: error.clone(),
            };
            finish(
                inner,
                id,
                |info| {
                    info.status = JobStatus::Failed;
                    info.error = Some(error);
                },
                &event,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn execution(cancelled: bool) -> JobResult {
        JobResult::Execution(ExecutionReport {
            checks: Vec::new(),
            outcomes: Vec::new(),
            cancelled,
            error: None,
            error_phrase: None,
            journal: None,
            notices: Vec::new(),
        })
    }

    #[test]
    fn writes_run_one_at_a_time_in_order() {
        let jobs = JobManager::new(EventBus::default());
        let running = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(Mutex::new(Vec::new()));
        let ids: Vec<JobId> = (0..4)
            .map(|n| {
                let (running, order) = (Arc::clone(&running), Arc::clone(&order));
                jobs.spawn(
                    JobKind::ExecutePlan,
                    &yaoplm_core::Phrase::title("plan").with("title", format!("job {n}")),
                    None,
                    Queue::Write,
                    move |_| {
                        assert_eq!(running.fetch_add(1, Ordering::SeqCst), 0);
                        thread::sleep(Duration::from_millis(10));
                        lock(&order).push(n);
                        running.fetch_sub(1, Ordering::SeqCst);
                        Ok(execution(false))
                    },
                )
            })
            .collect();
        for id in &ids {
            assert_eq!(jobs.wait(*id).unwrap().status, JobStatus::Finished);
        }
        assert_eq!(*lock(&order), [0, 1, 2, 3]);
        assert_eq!(jobs.list().len(), 4);
        assert!(jobs.list()[0].result.is_none());
        assert!(jobs.get(ids[0]).unwrap().result.is_some());
    }

    #[test]
    fn events_cancel_and_failures() {
        let bus = EventBus::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let _sub = bus.subscribe(move |event| {
            let name = serde_json::to_value(event).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_owned();
            lock(&log).push(name);
        });
        let jobs = JobManager::new(bus);
        let title = yaoplm_core::Phrase::title("plan").with("title", "copy");
        let id = jobs.spawn(JobKind::ExecutePlan, &title, None, Queue::Write, |ctx| {
            for done in 0..=10 {
                ctx.progress(done, Some(10), Some("Game"));
            }
            while !ctx.is_cancelled() {
                thread::sleep(Duration::from_millis(1));
            }
            Ok(execution(true))
        });
        thread::sleep(Duration::from_millis(20));
        jobs.cancel(id).unwrap();
        let info = jobs.wait(id).unwrap();
        assert_eq!(info.status, JobStatus::Cancelled);
        assert_eq!(info.done, 10);
        assert_eq!(info.item.as_deref(), Some("Game"));
        let events = lock(&seen).clone();
        assert_eq!(events.first().map(String::as_str), Some("Queued"));
        assert_eq!(events.last().map(String::as_str), Some("Cancelled"));
        // Throttled: the first and the final progress, not all eleven.
        assert!(
            events.iter().filter(|e| *e == "Progress").count() <= 3,
            "{events:?}"
        );

        let scan = yaoplm_core::Phrase::title("scanLibrary");
        let failed = jobs.spawn(JobKind::ScanLibrary, &scan, None, Queue::Read, |_| {
            Err(ApiError::not_found("gone"))
        });
        let info = jobs.wait(failed).unwrap();
        assert_eq!(info.status, JobStatus::Failed);
        assert_eq!(info.error.unwrap().code, ErrorCode::NotFound);

        let panics = jobs.spawn(JobKind::ScanLibrary, &scan, None, Queue::Read, |_| {
            panic!("boom")
        });
        assert_eq!(jobs.wait(panics).unwrap().status, JobStatus::Failed);
        assert!(jobs.cancel(JobId(999)).is_err());
    }
}
