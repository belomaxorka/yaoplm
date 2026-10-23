//! Finishing or undoing operations a crash or an unplugged device left
//! half done (SPEC §6, IMP-5).
//!
//! Every journal in `.yaoplm/journal/` becomes part of one recovery plan:
//! - a transaction past its commit point is finished: OPL may already show
//!   the change, so it is completed rather than undone;
//! - a transaction before its commit point is rolled back by default, or
//!   redone from where it stopped when the user asks to resume (the sources
//!   must still be there, unchanged);
//! - transactions that never started are dropped, or run on resume.
//!
//! Recovery steps are idempotent and run in resume mode, so recovery itself
//! may be interrupted and run again. A journal left by an interrupted
//! recovery is always finished first; the journal it was recovering is not
//! planned twice.

use std::collections::HashSet;
use std::io;

use yaoplm_device::fs::DeviceFs;

use crate::device::DeviceState;
use crate::exec::undo;
use crate::journal::{self, Journal, TxState};
use crate::plan::{Notice, Plan, PlanKind, Step, Transaction};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecoveryAction {
    /// Undo what was not committed, finish what was.
    #[default]
    Rollback,
    /// Redo what was not committed, then everything not started.
    Resume,
}

fn remap(step: &Step, base: usize) -> Step {
    match step {
        Step::WriteFile {
            path,
            source,
            offset,
            len,
        } => Step::WriteFile {
            path: path.clone(),
            source: source + base,
            offset: *offset,
            len: *len,
        },
        Step::WriteZso {
            path,
            source,
            max_len,
        } => Step::WriteZso {
            path: path.clone(),
            source: source + base,
            max_len: *max_len,
        },
        Step::RemoveSource { source } => Step::RemoveSource {
            source: source + base,
        },
        other => other.clone(),
    }
}

fn add_journal(plan: &mut Plan, journal: &Journal, action: RecoveryAction) {
    let base = plan.sources.len();
    plan.sources.extend(journal.plan.sources.iter().cloned());
    let action = if journal.plan.kind == PlanKind::Recovery {
        RecoveryAction::Resume
    } else {
        action
    };
    for (tx, progress) in journal.plan.transactions.iter().zip(&journal.progress) {
        let done = progress.done.min(tx.steps.len());
        let (steps, verb): (Vec<Step>, &str) = match (progress.state, action) {
            (TxState::Done | TxState::RolledBack, _) => continue,
            (TxState::Pending, RecoveryAction::Rollback) => continue,
            (TxState::Pending, RecoveryAction::Resume) => (tx.steps.clone(), "Run"),
            (TxState::Committed, _) | (TxState::Running, RecoveryAction::Resume) => {
                (tx.steps[done..].to_vec(), "Finish")
            }
            (TxState::Running, RecoveryAction::Rollback) => {
                // The step at `done` may have started; undoing it is safe.
                let end = (done + 1).min(tx.steps.len());
                let undone = tx.steps[..end].iter().rev().flat_map(undo).collect();
                (undone, "Undo")
            }
        };
        let mut recovery = Transaction::new(format!("{verb}: {}", tx.title));
        recovery.game = tx.game.clone();
        for step in &steps {
            recovery.then(remap(step, base));
        }
        if progress.state == TxState::Committed && action == RecoveryAction::Rollback {
            recovery.notices.push(Notice::info(
                "alreadyCommitted",
                "this change was already visible to OPL, so it is completed instead of undone",
            ));
        }
        plan.transactions.push(recovery);
    }
    let mut drop = Transaction::new(format!("Close journal {}", journal.id));
    drop.then(Step::DropJournal {
        id: journal.id.clone(),
    });
    plan.transactions.push(drop);
}

/// Plans recovering every unfinished operation on the device.
pub fn plan_recovery(fs: &dyn DeviceFs, action: RecoveryAction) -> io::Result<Plan> {
    let state = DeviceState::read(fs)?;
    let (journals, unreadable) = journal::load_all(fs)?;
    let mut plan = Plan::new(
        PlanKind::Recovery,
        "Recover unfinished operations",
        state.info.fingerprint.clone(),
        state.info.free_bytes,
    );
    for bad in &unreadable {
        plan.notices.push(Notice::error(
            "journalUnreadable",
            format!(
                "journal {} cannot be read ({}); check the device by hand",
                bad.file, bad.error
            ),
        ));
    }
    // A recovery that was interrupted already covers its journals.
    let covered: HashSet<&str> = journals
        .iter()
        .filter(|j| j.plan.kind == PlanKind::Recovery)
        .flat_map(|j| &j.plan.transactions)
        .flat_map(|t| &t.steps)
        .filter_map(|s| match s {
            Step::DropJournal { id } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    for journal in &journals {
        if !covered.contains(journal.id.as_str()) {
            add_journal(&mut plan, journal, action);
        }
    }
    // Covered journals are closed by the recovery that covers them; its own
    // DropJournal steps run when it is finished above.
    if journals.is_empty() && unreadable.is_empty() {
        plan.notices
            .push(Notice::info("nothingToRecover", "no unfinished operation"));
    }
    plan.title = match journals.len() - covered.len().min(journals.len()) {
        1 => "Recover 1 unfinished operation".into(),
        n => format!("Recover {n} unfinished operations"),
    };
    Ok(plan)
}
