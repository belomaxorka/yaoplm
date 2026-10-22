//! Runs a [`Plan`] on a device: journal, commit points, rollback (IMP-7,
//! IMP-9, SPEC §6).
//!
//! Before anything is written the executor re-checks what the plan assumed:
//! the same device (volume fingerprint), unchanged sources, no unfinished
//! journal, enough free space. Then each transaction runs step by step:
//! - before its commit point, any failure or a cancel undoes the steps done
//!   so far, newest first;
//! - after it, a failure leaves the journal behind so the rest can be
//!   finished later ([`crate::recovery`]).
//!
//! The batch stops at the first failure; transactions already done stay.

use std::io;
use std::path::PathBuf;

use yaoplm_device::fs::{DeviceFs, PART_SUFFIX, TMP_SUFFIX, write_atomic};
use yaoplm_device::{CancelToken, CopyError, RelPath, SourceFs, copy_exact};

use crate::journal::{self, Journal, TxProgress, TxState};
use crate::plan::{Expect, Plan, PlanKind, Removal, Step};
use crate::source::open_stream;

/// What a front end needs to draw progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub transaction: usize,
    pub transactions: usize,
    pub title: String,
    /// Bytes written by the whole plan so far, and in total.
    pub done: u64,
    pub total: u64,
}

/// How a transaction ended, as reported while the plan runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxEvent {
    Started,
    Done,
    RolledBack,
    Interrupted,
}

/// What happened to one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepEvent {
    Started,
    Done,
    Failed(String),
    /// Rolled back after a failure or a cancel.
    Undone,
}

/// Everything the executor reports while it runs: byte progress, plus the
/// start and end of every transaction and step for a live log in the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event<'a> {
    Progress(&'a Progress),
    Transaction {
        index: usize,
        title: &'a str,
        state: TxEvent,
    },
    Step {
        transaction: usize,
        index: usize,
        step: &'a Step,
        state: StepEvent,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("the plan cannot run: {0}")]
    Blocked(String),
    #[error("the device changed since the plan was made")]
    DeviceChanged { planned: String, now: String },
    #[error("{} changed since the plan was made", .0.display())]
    SourceChanged(PathBuf),
    #[error("an unfinished operation is recorded on this device (journal {0}); recover it first")]
    Unfinished(String),
    #[error("not enough free space: {needed} bytes needed, {free} available")]
    NoSpace { needed: u64, free: u64 },
    #[error("{0}")]
    Precondition(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: io::Error,
    },
    #[error("cancelled")]
    Cancelled,
}

impl ExecError {
    fn io(context: impl Into<String>) -> impl FnOnce(io::Error) -> Self {
        let context = context.into();
        move |source| Self::Io { context, source }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    RolledBack,
    NotStarted,
    /// Stopped half way; the journal on the device says what is left.
    Interrupted,
}

#[derive(Debug)]
pub struct Report {
    pub outcomes: Vec<Outcome>,
    pub cancelled: bool,
    pub error: Option<ExecError>,
    /// The journal left on the device when something is half done.
    pub journal: Option<String>,
}

impl Report {
    pub fn succeeded(&self) -> bool {
        self.error.is_none() && !self.cancelled
    }
}

pub struct Context<'a> {
    pub fs: &'a dyn DeviceFs,
    pub sources: &'a dyn SourceFs,
    pub cancel: &'a CancelToken,
}

/// How strictly steps check what they find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A fresh plan: every precondition must hold.
    Fresh,
    /// Recovery: steps already done are skipped.
    Resume,
}

fn rel_err(e: yaoplm_device::PathError) -> ExecError {
    ExecError::Precondition(e.to_string())
}

fn part_path(path: &RelPath) -> Result<RelPath, ExecError> {
    path.with_suffix(PART_SUFFIX).map_err(rel_err)
}

fn exists(fs: &dyn DeviceFs, path: &RelPath) -> Result<Option<yaoplm_device::FileMeta>, ExecError> {
    fs.metadata(path)
        .map_err(ExecError::io(format!("cannot check {path}")))
}

fn same_name(a: &RelPath, b: &RelPath) -> bool {
    a.as_str().to_lowercase() == b.as_str().to_lowercase()
}

fn remove_if_exists(fs: &dyn DeviceFs, path: &RelPath) -> Result<(), ExecError> {
    if exists(fs, path)?.is_some() {
        fs.remove_file(path)
            .map_err(ExecError::io(format!("cannot remove {path}")))?;
    }
    Ok(())
}

fn ensure_parent(fs: &dyn DeviceFs, path: &RelPath) -> Result<(), ExecError> {
    let mut dirs = Vec::new();
    let mut at = path.parent();
    while let Some(dir) = at.filter(|d| !d.is_root()) {
        at = dir.parent();
        dirs.push(dir);
    }
    for dir in dirs.iter().rev() {
        fs.create_dir(dir)
            .map_err(ExecError::io(format!("cannot create {dir}")))?;
    }
    Ok(())
}

/// The steps that undo `step`, for rollback and recovery.
pub fn undo(step: &Step) -> Vec<Step> {
    let ours = |path: &RelPath, len: u64| Step::Remove {
        path: path.clone(),
        len,
        what: Removal::Ours,
    };
    match step {
        Step::CreateDir { path } => vec![Step::RemoveDir { path: path.clone() }],
        Step::WriteFile { path, len, .. } => {
            let mut steps = Vec::new();
            if let Ok(part) = path.with_suffix(PART_SUFFIX) {
                steps.push(ours(&part, 0));
            }
            // Only a file of exactly this size is ours.
            steps.push(ours(path, *len));
            steps
        }
        Step::PutFile {
            path,
            content,
            expect,
        } => {
            let mut steps = Vec::new();
            if let Ok(tmp) = path.with_suffix(TMP_SUFFIX) {
                steps.push(ours(&tmp, 0));
            }
            match expect {
                Expect::Content(old) => steps.push(Step::PutFile {
                    path: path.clone(),
                    content: old.clone(),
                    expect: Expect::Any,
                }),
                Expect::Absent => steps.push(ours(path, content.0.len() as u64)),
                Expect::Any => {}
            }
            steps
        }
        Step::CopyFile { to, .. } => vec![ours(to, 0)],
        Step::Rename { from, to } => vec![Step::Rename {
            from: to.clone(),
            to: from.clone(),
        }],
        Step::RemoveDir { .. }
        | Step::Remove { .. }
        | Step::RemoveSource { .. }
        | Step::DropJournal { .. } => Vec::new(),
    }
}

struct Runner<'a, 'b> {
    ctx: &'a Context<'b>,
    plan: &'a Plan,
    mode: Mode,
    done: u64,
    total: u64,
    events: &'a mut dyn FnMut(Event<'_>),
    current: Progress,
}

impl Runner<'_, '_> {
    fn report(&mut self) {
        self.current.done = self.done;
        (self.events)(Event::Progress(&self.current));
    }

    fn step_event(&mut self, index: usize, step: &Step, state: StepEvent) {
        (self.events)(Event::Step {
            transaction: self.current.transaction,
            index,
            step,
            state,
        });
    }

    fn tx_event(&mut self, state: TxEvent) {
        (self.events)(Event::Transaction {
            index: self.current.transaction,
            title: &self.current.title,
            state,
        });
    }

    /// Runs one step; in resume mode a step already done is skipped. A
    /// precondition failure has no side effects.
    fn run(&mut self, step: &Step) -> Result<(), ExecError> {
        let fs = self.ctx.fs;
        match step {
            Step::CreateDir { path } => fs
                .create_dir(path)
                .map_err(ExecError::io(format!("cannot create {path}"))),
            Step::RemoveDir { path } => {
                if exists(fs, path)?.is_some_and(|m| m.is_dir) {
                    // A folder the user put files in stays.
                    let _ = fs.remove_dir(path);
                }
                Ok(())
            }
            Step::WriteFile {
                path,
                source,
                offset,
                len,
            } => self.write_file(path, *source, *offset, *len),
            Step::PutFile {
                path,
                content,
                expect,
            } => {
                let current = match exists(fs, path)? {
                    Some(_) => Some(
                        fs.read(path)
                            .map_err(ExecError::io(format!("cannot read {path}")))?,
                    ),
                    None => None,
                };
                if self.mode == Mode::Resume && current.as_deref() == Some(&content.0[..]) {
                    return Ok(());
                }
                let ok = match expect {
                    Expect::Absent => current.is_none(),
                    Expect::Content(old) => current.as_deref() == Some(&old.0[..]),
                    Expect::Any => true,
                };
                if !ok {
                    return Err(ExecError::Precondition(format!(
                        "{path} changed since the plan was made"
                    )));
                }
                write_atomic(fs, path, &content.0)
                    .map_err(ExecError::io(format!("cannot write {path}")))?;
                self.done += content.0.len() as u64;
                Ok(())
            }
            Step::CopyFile { from, to } => {
                let data = fs
                    .read(from)
                    .map_err(ExecError::io(format!("cannot read {from}")))?;
                if exists(fs, to)?.is_some() {
                    let same = fs.read(to).is_ok_and(|d| d == data);
                    if self.mode == Mode::Resume && same {
                        return Ok(());
                    }
                    return Err(ExecError::Precondition(format!("{to} already exists")));
                }
                ensure_parent(fs, to)?;
                write_atomic(fs, to, &data).map_err(ExecError::io(format!("cannot write {to}")))
            }
            Step::Rename { from, to } => {
                let source = exists(fs, from)?;
                let target = exists(fs, to)?;
                if self.mode == Mode::Resume && source.is_none() && target.is_some() {
                    return Ok(());
                }
                if source.is_none() {
                    return Err(ExecError::Precondition(format!("{from} does not exist")));
                }
                if target.is_some() && !same_name(from, to) {
                    return Err(ExecError::Precondition(format!("{to} already exists")));
                }
                fs.rename(from, to)
                    .map_err(ExecError::io(format!("cannot rename {from} to {to}")))?;
                fs.sync_dir(&to.parent().unwrap_or_default())
                    .map_err(ExecError::io(format!("cannot sync {to}")))
            }
            Step::Remove { path, len, what } => {
                let Some(meta) = exists(fs, path)? else {
                    return Ok(());
                };
                if *what == Removal::Ours && *len > 0 && meta.len != *len {
                    // Not the file this operation created: leave it.
                    return Ok(());
                }
                fs.remove_file(path)
                    .map_err(ExecError::io(format!("cannot remove {path}")))
            }
            Step::RemoveSource { source } => {
                let source = self.plan.sources.get(*source).ok_or_else(|| {
                    ExecError::Precondition("the plan names an unknown source".into())
                })?;
                for file in &source.files {
                    match self.ctx.sources.remove(&file.path) {
                        Err(err) if err.kind() != io::ErrorKind::NotFound => {
                            return Err(ExecError::Io {
                                context: format!("cannot remove {}", file.path.display()),
                                source: err,
                            });
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Step::DropJournal { id } => journal::remove(fs, id)
                .map_err(ExecError::io(format!("cannot remove journal {id}"))),
        }
    }

    fn write_file(
        &mut self,
        path: &RelPath,
        source: usize,
        offset: u64,
        len: u64,
    ) -> Result<(), ExecError> {
        let fs = self.ctx.fs;
        let part = part_path(path)?;
        if let Some(meta) = exists(fs, path)? {
            if self.mode == Mode::Resume && meta.len == len && exists(fs, &part)?.is_none() {
                self.done += len;
                return Ok(());
            }
            return Err(ExecError::Precondition(format!("{path} already exists")));
        }
        let source =
            self.plan.sources.get(source).ok_or_else(|| {
                ExecError::Precondition("the plan names an unknown source".into())
            })?;
        let mut reader = open_stream(self.ctx.sources, source, offset).map_err(ExecError::io(
            format!("cannot read {}", source.path.display()),
        ))?;
        remove_if_exists(fs, &part)?;
        let mut file = fs
            .create(&part)
            .map_err(ExecError::io(format!("cannot create {part}")))?;
        // Best effort: a refusal only costs contiguity.
        let _ = file.preallocate(len);
        let start = self.done;
        let result = {
            let events = &mut *self.events;
            let current = &mut self.current;
            copy_exact(&mut reader, &mut *file, len, self.ctx.cancel, &mut |n| {
                current.done = start + n;
                events(Event::Progress(current));
            })
        };
        match result {
            Ok(()) => {}
            Err(CopyError::Cancelled) => return Err(ExecError::Cancelled),
            Err(CopyError::Io(source)) => {
                return Err(ExecError::Io {
                    context: format!("cannot write {path}"),
                    source,
                });
            }
            Err(err @ CopyError::ShortSource { .. }) => {
                return Err(ExecError::Io {
                    context: format!("cannot write {path}"),
                    source: io::Error::new(io::ErrorKind::UnexpectedEof, err.to_string()),
                });
            }
        }
        file.sync()
            .map_err(ExecError::io(format!("cannot sync {part}")))?;
        drop(file);
        fs.rename(&part, path)
            .map_err(ExecError::io(format!("cannot rename {part}")))?;
        fs.sync_dir(&path.parent().unwrap_or_default())
            .map_err(ExecError::io(format!("cannot sync {path}")))?;
        self.done = start + len;
        Ok(())
    }

    /// Undoes `steps` newest first. Errors are returned after trying all.
    fn rollback(&mut self, steps: &[Step]) -> Result<(), ExecError> {
        let saved = self.mode;
        self.mode = Mode::Resume;
        let mut first_error = None;
        for (index, step) in steps.iter().enumerate().rev() {
            let mut failed = None;
            for undo_step in undo(step) {
                if let Err(err) = self.run(&undo_step) {
                    failed.get_or_insert(err.to_string());
                    first_error.get_or_insert(err);
                }
            }
            let state = match failed {
                None => StepEvent::Undone,
                Some(error) => StepEvent::Failed(error),
            };
            self.step_event(index, step, state);
        }
        self.mode = saved;
        first_error.map_or(Ok(()), Err)
    }
}

/// Sources read by the plan must be exactly as planned. Sources that are
/// only removed (finishing a move) may already be partly gone.
fn check_sources(ctx: &Context<'_>, plan: &Plan) -> Result<(), ExecError> {
    let steps = || plan.transactions.iter().flat_map(|t| &t.steps);
    for (index, source) in plan.sources.iter().enumerate() {
        let read = steps().any(|s| matches!(s, Step::WriteFile { source, .. } if *source == index));
        let removed =
            steps().any(|s| matches!(s, Step::RemoveSource { source } if *source == index));
        if !read && !removed {
            continue;
        }
        for file in &source.files {
            let meta = match ctx.sources.metadata(&file.path) {
                Ok(meta) => meta,
                Err(_) if !read => continue,
                Err(_) => return Err(ExecError::SourceChanged(file.path.clone())),
            };
            if meta.len != file.len || meta.modified != file.modified {
                return Err(ExecError::SourceChanged(file.path.clone()));
            }
        }
    }
    Ok(())
}

fn check_device(ctx: &Context<'_>, plan: &Plan) -> Result<u64, ExecError> {
    let info = ctx
        .fs
        .info()
        .map_err(ExecError::io("cannot query the device"))?;
    if info.fingerprint != plan.fingerprint {
        return Err(ExecError::DeviceChanged {
            planned: plan.fingerprint.clone(),
            now: info.fingerprint,
        });
    }
    Ok(info.free_bytes)
}

/// Everything that must hold before the first write.
fn preflight(ctx: &Context<'_>, plan: &Plan) -> Result<(), ExecError> {
    if let Some(blocker) = plan.blockers().next() {
        return Err(ExecError::Blocked(blocker.message.clone()));
    }
    let free = check_device(ctx, plan)?;
    journal::clean_temp(ctx.fs).map_err(ExecError::io("cannot clean up the journal folder"))?;
    let (journals, unreadable) =
        journal::load_all(ctx.fs).map_err(ExecError::io("cannot read the journals"))?;
    if let Some(bad) = unreadable.first() {
        return Err(ExecError::Unfinished(bad.file.clone()));
    }
    let resolved: Vec<&str> = plan
        .transactions
        .iter()
        .flat_map(|t| &t.steps)
        .filter_map(|s| match s {
            Step::DropJournal { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    if let Some(open) = journals.iter().find(|j| !resolved.contains(&j.id.as_str())) {
        return Err(ExecError::Unfinished(open.id.clone()));
    }
    check_sources(ctx, plan)?;
    let needed = plan.write_bytes();
    if needed > free {
        return Err(ExecError::NoSpace { needed, free });
    }
    Ok(())
}

/// Runs `plan`. Never panics on device errors: the report says what
/// happened and whether a journal was left for recovery.
pub fn execute(plan: &Plan, ctx: &Context<'_>, progress: &mut dyn FnMut(&Progress)) -> Report {
    execute_with_events(plan, ctx, &mut |event| {
        if let Event::Progress(p) = event {
            progress(p);
        }
    })
}

/// [`execute`], reporting every transaction and step as well as progress.
pub fn execute_with_events(
    plan: &Plan,
    ctx: &Context<'_>,
    events: &mut dyn FnMut(Event<'_>),
) -> Report {
    let count = plan.transactions.len();
    let mut report = Report {
        outcomes: vec![Outcome::NotStarted; count],
        cancelled: false,
        error: None,
        journal: None,
    };
    if let Err(err) = preflight(ctx, plan) {
        report.error = Some(err);
        return report;
    }
    let mut journal = Journal::new(plan);
    if let Err(err) = journal::save(ctx.fs, &journal) {
        report.error = Some(ExecError::Io {
            context: "cannot write the journal".into(),
            source: err,
        });
        return report;
    }
    let mode = if plan.kind == PlanKind::Recovery {
        Mode::Resume
    } else {
        Mode::Fresh
    };
    let mut runner = Runner {
        ctx,
        plan,
        mode,
        done: 0,
        total: plan.write_bytes(),
        events,
        current: Progress {
            transaction: 0,
            transactions: count,
            title: String::new(),
            done: 0,
            total: plan.write_bytes(),
        },
    };
    runner.current.total = runner.total;

    for (index, tx) in plan.transactions.iter().enumerate() {
        if ctx.cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        if index > 0
            && let Err(err) = check_device(ctx, plan)
        {
            report.error = Some(err);
            break;
        }
        runner.current.transaction = index;
        runner.current.title = tx.title.clone();
        runner.report();
        runner.tx_event(TxEvent::Started);
        let committed_at_start = tx.commit == 0;
        journal.progress[index] = TxProgress {
            state: if committed_at_start {
                TxState::Committed
            } else {
                TxState::Running
            },
            done: 0,
        };
        let mut failure = journal::save(ctx.fs, &journal)
            .err()
            .map(|e| (0, ExecError::io("cannot write the journal")(e), false));

        if failure.is_none() {
            for (i, step) in tx.steps.iter().enumerate() {
                if ctx.cancel.is_cancelled() && i < tx.commit {
                    failure = Some((i, ExecError::Cancelled, false));
                    break;
                }
                runner.step_event(i, step, StepEvent::Started);
                if let Err(err) = runner.run(step) {
                    runner.step_event(i, step, StepEvent::Failed(err.to_string()));
                    // A precondition failure did nothing; anything else may
                    // have left a partial result.
                    let touched = !matches!(err, ExecError::Precondition(_));
                    failure = Some((i, err, touched));
                    break;
                }
                runner.step_event(i, step, StepEvent::Done);
                journal.progress[index].done = i + 1;
                if i + 1 == tx.commit {
                    journal.progress[index].state = TxState::Committed;
                }
                if let Err(err) = journal::save(ctx.fs, &journal) {
                    failure = Some((i + 1, ExecError::io("cannot write the journal")(err), false));
                    break;
                }
            }
        }

        match failure {
            None => {
                journal.progress[index].state = TxState::Done;
                report.outcomes[index] = Outcome::Done;
                runner.tx_event(TxEvent::Done);
                if let Err(err) = journal::save(ctx.fs, &journal) {
                    report.error = Some(ExecError::io("cannot write the journal")(err));
                    report.journal = Some(journal.id.clone());
                    return report;
                }
            }
            Some((at, err, touched)) if at < tx.commit => {
                let end = if touched { at + 1 } else { at };
                let rolled = runner.rollback(&tx.steps[..end.min(tx.steps.len())]);
                report.cancelled = matches!(err, ExecError::Cancelled);
                if !report.cancelled {
                    report.error = Some(err);
                }
                match rolled {
                    Ok(()) => {
                        journal.progress[index] = TxProgress {
                            state: TxState::RolledBack,
                            done: 0,
                        };
                        report.outcomes[index] = Outcome::RolledBack;
                        runner.tx_event(TxEvent::RolledBack);
                        if journal::save(ctx.fs, &journal).is_err() {
                            report.journal = Some(journal.id.clone());
                            return report;
                        }
                    }
                    Err(rollback_err) => {
                        report.outcomes[index] = Outcome::Interrupted;
                        runner.tx_event(TxEvent::Interrupted);
                        report.journal = Some(journal.id.clone());
                        report.error.get_or_insert(rollback_err);
                        return report;
                    }
                }
                break;
            }
            Some((_, err, _)) => {
                report.outcomes[index] = Outcome::Interrupted;
                runner.tx_event(TxEvent::Interrupted);
                report.error = Some(err);
                report.journal = Some(journal.id.clone());
                return report;
            }
        }
    }

    if journal.is_settled() {
        if let Err(err) = journal::remove(ctx.fs, &journal.id) {
            report
                .error
                .get_or_insert(ExecError::io("cannot remove the journal")(err));
            report.journal = Some(journal.id.clone());
        }
    } else {
        report.journal = Some(journal.id.clone());
    }
    report
}
