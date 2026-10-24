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

use std::io::{self, BufReader, BufWriter, Write};
use std::path::PathBuf;

use yaoplm_device::copy::BLOCK_SIZE;
use yaoplm_device::fs::{DeviceFs, PART_SUFFIX, TMP_SUFFIX, write_atomic};
use yaoplm_device::{CancelToken, CopyError, RelPath, SourceFs, copy_exact};
use yaoplm_formats::vmc;
use yaoplm_formats::zso::{self, ZsoError};
use yaoplm_i18n::Phrase;

use crate::checks::{Checked, Taps};
use crate::journal::{self, Journal, TxProgress, TxState};
use crate::plan::{Expect, Notice, Plan, PlanKind, Removal, Step};
use crate::source::{device_path, open_tapped_stream};

/// What a front end needs to draw progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub transaction: usize,
    pub transactions: usize,
    pub title: String,
    pub title_phrase: Option<Phrase>,
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
    Failed(Phrase),
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
        title_phrase: Option<&'a Phrase>,
        state: TxEvent,
    },
    Step {
        transaction: usize,
        index: usize,
        step: &'a Step,
        state: StepEvent,
    },
    /// A transaction that wrote a source marked for a dump check is done,
    /// and these are the hashes of what it wrote (GAME-5).
    Checked {
        transaction: usize,
        source: usize,
        checked: &'a Checked,
    },
    /// Something a step found once done, such as a new VMC the volume could
    /// only give in several fragments (VMC-1).
    Notice {
        transaction: usize,
        notice: &'a Notice,
    },
}

/// Why a plan stopped. [`ExecError::phrase`] says it in the user's
/// language (ADR-0029); `Display` is its English text.
#[derive(Debug)]
pub enum ExecError {
    /// A plan notice of level error.
    Blocked(Box<Notice>),
    DeviceChanged {
        planned: String,
        now: String,
    },
    SourceChanged(PathBuf),
    Unfinished(String),
    NoSpace {
        needed: u64,
        free: u64,
    },
    Precondition(Phrase),
    Io {
        /// What was being done, such as `errors.cannotWrite`.
        action: Phrase,
        source: io::Error,
    },
    Cancelled,
}

impl ExecError {
    fn io(action: Phrase) -> impl FnOnce(io::Error) -> Self {
        move |source| Self::Io { action, source }
    }

    pub fn phrase(&self) -> Phrase {
        match self {
            Self::Blocked(notice) => Phrase::error("planBlocked").with("reason", notice.said()),
            Self::DeviceChanged { .. } => Phrase::error("deviceChanged"),
            Self::SourceChanged(path) => {
                Phrase::error("sourceChanged").with("path", path.display().to_string())
            }
            Self::Unfinished(id) => Phrase::error("unfinishedJournal").with("id", id),
            Self::NoSpace { needed, free } => Phrase::error("noSpace")
                .with("needed", *needed)
                .with("free", *free),
            Self::Precondition(phrase) => phrase.clone(),
            Self::Io { action, source } => Phrase::error("io")
                .with("action", action.clone())
                .with("error", source.to_string()),
            Self::Cancelled => Phrase::error("cancelled"),
        }
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.phrase().english())
    }
}

impl std::error::Error for ExecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// `errors.<code>` about `path`, for [`ExecError::io`].
fn doing(code: &str, path: &dyn std::fmt::Display) -> Phrase {
    Phrase::error(code).with("path", path.to_string())
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
    /// Hashes taken while writing, by transaction (GAME-5).
    pub checked: Vec<(usize, Checked)>,
    /// What steps found once done, by transaction.
    pub notices: Vec<(usize, Notice)>,
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
    ExecError::Precondition(e.phrase())
}

fn part_path(path: &RelPath) -> Result<RelPath, ExecError> {
    path.with_suffix(PART_SUFFIX).map_err(rel_err)
}

fn exists(fs: &dyn DeviceFs, path: &RelPath) -> Result<Option<yaoplm_device::FileMeta>, ExecError> {
    fs.metadata(path)
        .map_err(ExecError::io(doing("cannotCheck", &path)))
}

fn same_name(a: &RelPath, b: &RelPath) -> bool {
    a.as_str().to_lowercase() == b.as_str().to_lowercase()
}

/// The images and `ul` chunks `steps` leave on the device, under the names
/// later renames give them, for the fragment check (IMP-7b).
fn written_files(steps: &[Step]) -> Vec<RelPath> {
    let mut files: Vec<RelPath> = Vec::new();
    for step in steps {
        match step {
            Step::WriteFile { path, .. } | Step::WriteZso { path, .. } => files.push(path.clone()),
            Step::Rename { from, to } => {
                for file in files.iter_mut().filter(|f| same_name(f, from)) {
                    *file = to.clone();
                }
            }
            Step::Remove { path, .. } => files.retain(|f| !same_name(f, path)),
            _ => {}
        }
    }
    files
}

fn remove_if_exists(fs: &dyn DeviceFs, path: &RelPath) -> Result<(), ExecError> {
    if exists(fs, path)?.is_some() {
        fs.remove_file(path)
            .map_err(ExecError::io(doing("cannotRemove", &path)))?;
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
            .map_err(ExecError::io(doing("cannotCreate", &dir)))?;
    }
    Ok(())
}

/// The steps that undo `step` of `plan`, for rollback and recovery.
pub fn undo(step: &Step, plan: &Plan) -> Vec<Step> {
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
        Step::WriteZso { path, source, .. } => {
            let mut steps = Vec::new();
            if let Ok(part) = path.with_suffix(PART_SUFFIX) {
                steps.push(ours(&part, 0));
            }
            // The size is unknown up front: the header tells our ZSO from
            // a file that was there before (the step refuses to replace
            // one, but recovery undoes a step that may not have started).
            if let Some(source) = plan.sources.get(*source) {
                steps.push(Step::Remove {
                    path: path.clone(),
                    len: 0,
                    what: Removal::OursZso {
                        iso_len: source.len,
                    },
                });
            }
            steps
        }
        Step::WriteVmc { path, size_mb, .. } => {
            let mut steps = Vec::new();
            if let Ok(part) = path.with_suffix(PART_SUFFIX) {
                steps.push(ours(&part, 0));
            }
            // Only a file of exactly the card's size is ours.
            steps.push(ours(path, u64::from(*size_mb) << 20));
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
    /// Where the last write left its source: the source index, the offset
    /// reached and the open reader. The next `ul` chunk of that source reads
    /// on from it; opening the source again would mean, for an archive
    /// member, decoding the image from the start once more.
    stream: Option<(usize, u64, Box<dyn io::Read + Send>)>,
    /// Hashers of the source being written, when the plan checks it.
    taps: Option<Taps>,
    /// Hashes of the source the current transaction wrote whole.
    checked: Option<(usize, Checked)>,
    /// What the current transaction's steps found.
    found: Vec<Notice>,
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
            title_phrase: self.current.title_phrase.as_ref(),
            state,
        });
    }

    /// Runs one step; in resume mode a step already done is skipped. A
    /// precondition failure has no side effects.
    fn run(&mut self, step: &Step) -> Result<(), ExecError> {
        let fs = self.ctx.fs;
        // Only a `ul` chunk right after the one before it keeps the stream;
        // any other step closes it, so no file is deleted while still open
        // (on FAT, Windows keeps the name of such a file until it is closed).
        if !matches!(step, Step::WriteFile { .. }) {
            self.stream = None;
        }
        match step {
            Step::CreateDir { path } => fs
                .create_dir(path)
                .map_err(ExecError::io(doing("cannotCreate", &path))),
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
            Step::WriteZso {
                path,
                source,
                max_len,
            } => self.write_zso(path, *source, *max_len),
            Step::WriteVmc {
                path,
                size_mb,
                created,
            } => self.write_vmc(path, *size_mb, *created),
            Step::PutFile {
                path,
                content,
                expect,
            } => {
                let current = match exists(fs, path)? {
                    Some(_) => Some(
                        fs.read(path)
                            .map_err(ExecError::io(doing("cannotRead", &path)))?,
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
                    return Err(ExecError::Precondition(
                        Phrase::error("sourceChanged").with("path", path.to_string()),
                    ));
                }
                write_atomic(fs, path, &content.0)
                    .map_err(ExecError::io(doing("cannotWrite", &path)))?;
                self.done += content.0.len() as u64;
                Ok(())
            }
            Step::CopyFile { from, to } => {
                let data = fs
                    .read(from)
                    .map_err(ExecError::io(doing("cannotRead", &from)))?;
                if exists(fs, to)?.is_some() {
                    let same = fs.read(to).is_ok_and(|d| d == data);
                    if self.mode == Mode::Resume && same {
                        return Ok(());
                    }
                    return Err(ExecError::Precondition(
                        Phrase::error("alreadyExists").with("path", to.to_string()),
                    ));
                }
                ensure_parent(fs, to)?;
                write_atomic(fs, to, &data).map_err(ExecError::io(doing("cannotWrite", &to)))
            }
            Step::Rename { from, to } => {
                let source = exists(fs, from)?;
                let target = exists(fs, to)?;
                if self.mode == Mode::Resume && source.is_none() && target.is_some() {
                    return Ok(());
                }
                if source.is_none() {
                    return Err(ExecError::Precondition(
                        Phrase::error("doesNotExist").with("path", from.to_string()),
                    ));
                }
                if target.is_some() && !same_name(from, to) {
                    return Err(ExecError::Precondition(
                        Phrase::error("alreadyExists").with("path", to.to_string()),
                    ));
                }
                fs.rename(from, to).map_err(ExecError::io(
                    Phrase::error("cannotRename")
                        .with("from", from.to_string())
                        .with("to", to.to_string()),
                ))?;
                fs.sync_dir(&to.parent().unwrap_or_default())
                    .map_err(ExecError::io(doing("cannotSync", &to)))
            }
            Step::Remove { path, len, what } => {
                let Some(meta) = exists(fs, path)? else {
                    return Ok(());
                };
                let ours = match what {
                    Removal::Ours => *len == 0 || meta.len == *len,
                    Removal::OursZso { iso_len } => holds_zso(fs, path, *iso_len),
                    Removal::UserData | Removal::Cache => true,
                };
                if !ours {
                    // Not the file this operation created: leave it.
                    return Ok(());
                }
                fs.remove_file(path)
                    .map_err(ExecError::io(doing("cannotRemove", &path)))
            }
            Step::RemoveSource { source } => {
                let source = self
                    .plan
                    .sources
                    .get(*source)
                    .ok_or_else(|| ExecError::Precondition(Phrase::error("unknownSource")))?;
                for file in &source.files {
                    match self.ctx.sources.remove(&file.path) {
                        Err(err) if err.kind() != io::ErrorKind::NotFound => {
                            return Err(ExecError::Io {
                                action: doing("cannotRemove", &file.path.display()),
                                source: err,
                            });
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Step::DropJournal { id } => journal::remove(fs, id).map_err(ExecError::io(
                Phrase::error("cannotRemoveJournal").with("id", id),
            )),
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
            return Err(ExecError::Precondition(
                Phrase::error("alreadyExists").with("path", path.to_string()),
            ));
        }
        let index = source;
        let source = self
            .plan
            .sources
            .get(source)
            .ok_or_else(|| ExecError::Precondition(Phrase::error("unknownSource")))?;
        let mut reader = match self.stream.take() {
            Some((at_source, at, reader)) if at_source == index && at == offset => reader,
            _ => {
                // Hashes must cover the whole source in one pass: only a
                // fresh write from its first byte is checked.
                self.taps = source
                    .check
                    .filter(|_| self.mode == Mode::Fresh && offset == 0)
                    .map(|check| Taps::new(index, check, &source.kind));
                let raw = self.taps.as_ref().and_then(Taps::track);
                let stream = open_tapped_stream(self.ctx.sources, fs, source, offset, raw)
                    .map_err(ExecError::io(doing("cannotRead", &source.path.display())))?;
                match &self.taps {
                    Some(taps) => Box::new(taps.reader(stream)),
                    None => stream,
                }
            }
        };
        remove_if_exists(fs, &part)?;
        let mut file = fs
            .create(&part)
            .map_err(ExecError::io(doing("cannotCreate", &part)))?;
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
                    action: doing("cannotWrite", &path),
                    source,
                });
            }
            Err(err @ CopyError::ShortSource { .. }) => {
                return Err(ExecError::Io {
                    action: doing("cannotWrite", &path),
                    source: io::Error::new(io::ErrorKind::UnexpectedEof, err.to_string()),
                });
            }
        }
        file.sync()
            .map_err(ExecError::io(doing("cannotSync", &part)))?;
        drop(file);
        fs.rename(&part, path).map_err(ExecError::io(
            Phrase::error("cannotRename")
                .with("from", part.to_string())
                .with("to", path.to_string()),
        ))?;
        fs.sync_dir(&path.parent().unwrap_or_default())
            .map_err(ExecError::io(doing("cannotSync", &path)))?;
        self.done = start + len;
        self.stream = Some((index, offset + len, reader));
        if offset + len == source.len {
            self.finish_check(index);
        }
        Ok(())
    }

    /// Takes the hashes of source `index` if it was checked while written.
    fn finish_check(&mut self, index: usize) {
        let Some(taps) = self.taps.take_if(|t| t.source == index) else {
            return;
        };
        if let Some(checked) = self.plan.sources.get(index).and_then(|s| taps.finish(s)) {
            self.checked = Some((index, checked));
        }
    }

    /// Compresses the ISO sectors of `source` into a new ZSO at `path`
    /// (GAME-4). The index is written last, so the file only becomes a
    /// valid ZSO once complete; it is renamed into place after a sync.
    fn write_zso(&mut self, path: &RelPath, source: usize, max_len: u64) -> Result<(), ExecError> {
        let fs = self.ctx.fs;
        let part = part_path(path)?;
        let index = source;
        let source = self
            .plan
            .sources
            .get(source)
            .ok_or_else(|| ExecError::Precondition(Phrase::error("unknownSource")))?;
        if exists(fs, path)?.is_some() {
            // Done before an interruption only if it is our ZSO: something
            // else there must not pass for it (the old image goes next).
            if self.mode == Mode::Resume
                && exists(fs, &part)?.is_none()
                && holds_zso(fs, path, source.len)
            {
                self.done += max_len;
                return Ok(());
            }
            return Err(ExecError::Precondition(
                Phrase::error("alreadyExists").with("path", path.to_string()),
            ));
        }
        self.taps = source
            .check
            .filter(|_| self.mode == Mode::Fresh)
            .map(|check| Taps::new(index, check, &source.kind));
        let raw = self.taps.as_ref().and_then(Taps::track);
        let reader = open_tapped_stream(self.ctx.sources, fs, source, 0, raw)
            .map_err(ExecError::io(doing("cannotRead", &source.path.display())))?;
        // The compressor reads 2 KiB and writes a block at a time. The hasher
        // sits above the buffer, which may read past the image's end.
        let reader = BufReader::with_capacity(BLOCK_SIZE, reader);
        let mut reader: Box<dyn io::Read + Send> = match &self.taps {
            Some(taps) => Box::new(taps.reader(reader)),
            None => Box::new(reader),
        };
        remove_if_exists(fs, &part)?;
        let mut file = fs
            .create(&part)
            .map_err(ExecError::io(doing("cannotCreate", &part)))?;
        let start = self.done;
        let result = {
            let events = &mut *self.events;
            let current = &mut self.current;
            let cancel = self.ctx.cancel;
            let mut out = BufWriter::with_capacity(BLOCK_SIZE, &mut *file);
            let mut reported = 0u64;
            let compressed = zso::compress_with(
                &mut reader,
                source.len,
                &mut out,
                &|| cancel.is_cancelled(),
                &mut |n| {
                    if n - reported >= BLOCK_SIZE as u64 || n == source.len {
                        reported = n;
                        current.done = start + n;
                        events(Event::Progress(current));
                    }
                },
            );
            let flushed = out.flush();
            compressed.and_then(|stats| flushed.map(|()| stats).map_err(ZsoError::Io))
        };
        match result {
            Ok(_) => {}
            Err(ZsoError::Stopped) => return Err(ExecError::Cancelled),
            Err(ZsoError::Io(source)) => {
                return Err(ExecError::Io {
                    action: doing("cannotWrite", &path),
                    source,
                });
            }
            Err(other) => {
                return Err(ExecError::Io {
                    action: doing("cannotWrite", &path),
                    source: io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
                });
            }
        }
        file.sync()
            .map_err(ExecError::io(doing("cannotSync", &part)))?;
        drop(file);
        fs.rename(&part, path).map_err(ExecError::io(
            Phrase::error("cannotRename")
                .with("from", part.to_string())
                .with("to", path.to_string()),
        ))?;
        fs.sync_dir(&path.parent().unwrap_or_default())
            .map_err(ExecError::io(doing("cannotSync", &path)))?;
        self.done = start + max_len;
        self.finish_check(index);
        Ok(())
    }

    /// Formats a blank VMC at `path` (VMC-1). The space is reserved before
    /// the first byte, so the card lands in one run of clusters when the
    /// volume has one; a card in several fragments is reported, since OPL
    /// then asks to fall back on the real memory card.
    fn write_vmc(&mut self, path: &RelPath, size_mb: u32, created: i64) -> Result<(), ExecError> {
        let fs = self.ctx.fs;
        let part = part_path(path)?;
        let layout = vmc::Layout::new(size_mb).map_err(|_| {
            ExecError::Precondition(Phrase::error("vmcSize").with("size", u64::from(size_mb)))
        })?;
        let len = layout.bytes();
        if let Some(meta) = exists(fs, path)? {
            if self.mode == Mode::Resume && meta.len == len && exists(fs, &part)?.is_none() {
                self.done += len;
                return Ok(());
            }
            return Err(ExecError::Precondition(
                Phrase::error("alreadyExists").with("path", path.to_string()),
            ));
        }
        remove_if_exists(fs, &part)?;
        let mut file = fs
            .create(&part)
            .map_err(ExecError::io(doing("cannotCreate", &part)))?;
        // Best effort: a refusal only costs contiguity, which is checked below.
        let _ = file.preallocate(len);
        let start = self.done;
        let result = {
            let events = &mut *self.events;
            let current = &mut self.current;
            let cancel = self.ctx.cancel;
            let mut out = BufWriter::with_capacity(BLOCK_SIZE, &mut *file);
            let written = vmc::write_blank(
                &mut out,
                &layout,
                vmc::McTime::from_unix(created),
                &mut |n| {
                    current.done = start + n;
                    events(Event::Progress(current));
                    !cancel.is_cancelled()
                },
            );
            written.and_then(|()| out.flush())
        };
        match result {
            Ok(()) => {}
            Err(_) if self.ctx.cancel.is_cancelled() => return Err(ExecError::Cancelled),
            Err(source) => {
                return Err(ExecError::Io {
                    action: doing("cannotWrite", &path),
                    source,
                });
            }
        }
        file.sync()
            .map_err(ExecError::io(doing("cannotSync", &part)))?;
        drop(file);
        fs.rename(&part, path).map_err(ExecError::io(
            Phrase::error("cannotRename")
                .with("from", part.to_string())
                .with("to", path.to_string()),
        ))?;
        fs.sync_dir(&path.parent().unwrap_or_default())
            .map_err(ExecError::io(doing("cannotSync", &path)))?;
        self.done = start + len;
        if let Ok(Some(runs)) = fs.fragments(path)
            && runs > 1
        {
            self.found.push(Notice::warning(
                Phrase::notice("vmcFragmented")
                    .with("path", path.to_string())
                    .with("fragments", u64::from(runs)),
            ));
        }
        Ok(())
    }

    /// Undoes `steps` newest first. Errors are returned after trying all.
    fn rollback(&mut self, steps: &[Step]) -> Result<(), ExecError> {
        let saved = self.mode;
        self.mode = Mode::Resume;
        let mut first_error = None;
        for (index, step) in steps.iter().enumerate().rev() {
            let mut failed = None;
            for undo_step in undo(step, self.plan) {
                if let Err(err) = self.run(&undo_step) {
                    failed.get_or_insert(err.phrase());
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

/// A ZSO whose header says it holds `iso_len` bytes of ISO.
fn holds_zso(fs: &dyn DeviceFs, path: &RelPath, iso_len: u64) -> bool {
    let mut header = [0u8; zso::HEADER_SIZE];
    fs.open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| zso::ZsoHeader::parse(&header).is_some_and(|h| h.total_bytes == iso_len))
}

/// Sources read by the plan must be exactly as planned. Sources that are
/// only removed (finishing a move) may already be partly gone.
fn check_sources(ctx: &Context<'_>, plan: &Plan) -> Result<(), ExecError> {
    let steps = || plan.transactions.iter().flat_map(|t| &t.steps);
    for (index, source) in plan.sources.iter().enumerate() {
        let read = steps().any(|s| {
            matches!(s, Step::WriteFile { source, .. } | Step::WriteZso { source, .. } if *source == index)
        });
        let removed =
            steps().any(|s| matches!(s, Step::RemoveSource { source } if *source == index));
        if !read && !removed {
            continue;
        }
        if source.on_device {
            // The device has no times to compare; the size must hold.
            for file in &source.files {
                let path = device_path(&file.path)
                    .map_err(ExecError::io(Phrase::error("badSourcePath")))?;
                match exists(ctx.fs, &path)? {
                    Some(meta) if !meta.is_dir && meta.len == file.len => {}
                    _ => return Err(ExecError::SourceChanged(file.path.clone())),
                }
            }
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
        .map_err(ExecError::io(Phrase::error("cannotQueryDevice")))?;
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
        return Err(ExecError::Blocked(Box::new(blocker.clone())));
    }
    let free = check_device(ctx, plan)?;
    journal::clean_temp(ctx.fs).map_err(ExecError::io(Phrase::error("cannotCleanJournals")))?;
    let (journals, unreadable) =
        journal::load_all(ctx.fs).map_err(ExecError::io(Phrase::error("cannotReadJournals")))?;
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
    let needed = plan.peak_write_bytes();
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
        checked: Vec::new(),
        notices: Vec::new(),
    };
    if let Err(err) = preflight(ctx, plan) {
        report.error = Some(err);
        return report;
    }
    let mut journal = Journal::new(plan);
    if let Err(err) = journal::save(ctx.fs, &journal) {
        report.error = Some(ExecError::Io {
            action: Phrase::error("cannotWriteJournal"),
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
            title_phrase: None,
            done: 0,
            total: plan.write_bytes(),
        },
        stream: None,
        taps: None,
        checked: None,
        found: Vec::new(),
    };
    runner.current.total = runner.total;
    // OPL's fragment limits hold on BDM only (IMP-7b).
    let bdm = crate::fragments::applies(ctx.fs);
    let profile = plan.profile.unwrap_or_default();

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
        runner.checked = None;
        runner.found.clear();
        runner.current.title = tx.title.clone();
        runner.current.title_phrase = tx.title_phrase.clone();
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
        let mut failure = journal::save(ctx.fs, &journal).err().map(|e| {
            (
                0,
                ExecError::io(Phrase::error("cannotWriteJournal"))(e),
                false,
            )
        });

        if failure.is_none() {
            for (i, step) in tx.steps.iter().enumerate() {
                if ctx.cancel.is_cancelled() && i < tx.commit {
                    failure = Some((i, ExecError::Cancelled, false));
                    break;
                }
                runner.step_event(i, step, StepEvent::Started);
                if let Err(err) = runner.run(step) {
                    runner.step_event(i, step, StepEvent::Failed(err.phrase()));
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
                    failure = Some((
                        i + 1,
                        ExecError::io(Phrase::error("cannotWriteJournal"))(err),
                        false,
                    ));
                    break;
                }
            }
        }

        match failure {
            None => {
                journal.progress[index].state = TxState::Done;
                report.outcomes[index] = Outcome::Done;
                runner.tx_event(TxEvent::Done);
                let written = if bdm {
                    written_files(&tx.steps)
                } else {
                    Vec::new()
                };
                if !written.is_empty() {
                    runner
                        .found
                        .extend(crate::fragments::written_notice(ctx.fs, &written, profile));
                }
                if let Some((source, checked)) = runner.checked.take() {
                    (runner.events)(Event::Checked {
                        transaction: index,
                        source,
                        checked: &checked,
                    });
                    report.checked.push((index, checked));
                }
                for notice in std::mem::take(&mut runner.found) {
                    (runner.events)(Event::Notice {
                        transaction: index,
                        notice: &notice,
                    });
                    report.notices.push((index, notice));
                }
                if let Err(err) = journal::save(ctx.fs, &journal) {
                    report.error = Some(ExecError::io(Phrase::error("cannotWriteJournal"))(err));
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
                // A recovery goes on: its transactions stand alone, and the
                // journals it closes last are settled once one of them is
                // rolled back (a next recovery would only fail the same way).
                if plan.kind != PlanKind::Recovery || report.cancelled {
                    break;
                }
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

    // The last step may have reported less than it counts for (a ZSO is
    // counted at its worst case): end on the true total.
    runner.report();
    if journal.is_settled() {
        if let Err(err) = journal::remove(ctx.fs, &journal.id) {
            report
                .error
                .get_or_insert(ExecError::io(Phrase::error("cannotDropJournal"))(err));
            report.journal = Some(journal.id.clone());
        }
    } else {
        report.journal = Some(journal.id.clone());
    }
    report
}
