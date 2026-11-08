use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::fleet::claims::{self, Rows};
use crate::fleet::model::{Account, LaunchRequest, SessionRecord, Status};
use crate::fleet::records::{CLAIMS_RELEASED, CLAIMS_SWEEP_LOG, read_record, write_record};
use crate::fleet::runtime::Opening;

use super::{Fleet, session_running};

/// How many owners one `threads holder` read names, so a fleet with hundreds of exited sessions never asks for them all in a read the ledger cannot answer within its bound.
const OWNERS_PER_READ: usize = 64;

impl Fleet {
    /// Claims every task of a launch before its session starts: a live holder refuses it. A `--test` session claims nothing.
    pub(super) fn claim_before_start(&self, request: &LaunchRequest, tasks: &[String], verification: bool) -> NorthResult<()> {
        if verification {
            return Ok(());
        }
        claims::hold_all(self.claims.as_ref(), &claims::own_tasks(self.claims.as_ref(), tasks, &self.ancestors(request.parent.as_deref())), &request.name)
    }

    /// Starts a fresh session, giving its claims back when it does not start.
    pub(super) fn start_claiming(&self, record: &mut SessionRecord, account: &Account, opening: Opening) -> NorthResult<()> {
        let started = self.start(record, account, opening);
        if started.is_err() {
            claims::release_for(self.claims.as_ref(), record);
        }
        started
    }
}

impl Fleet {
    /// Adds `task` to the issues the running session `name` holds, as a launch's `--task` would have: the session's record names it, so `north fleet list --json` shows it to every dispatch whatever the threads ledger says, and its claim in the ledger is taken on top. A ledger that cannot be read is a warning, never a failure, because the record already holds the issue.
    pub fn hold_task(&self, name: &str, task: &str) -> NorthResult<()> {
        let session_dir = self.session_dir(name)?;
        let mut record = read_record(&session_dir)?;
        if record.verification {
            return Ok(());
        }
        if !record.tasks().any(|held| held == task) {
            if record.task_ref.is_none() {
                record.task_ref = Some(task.to_owned());
            } else {
                record.extra_tasks.push(task.to_owned());
            }
            write_record(&session_dir, &record)?;
        }
        match self.claims.hold(task, name) {
            Ok(Some(moved)) => println!("{moved}"),
            Ok(None) => println!("{name} holds {task}"),
            Err(error) => eprintln!("north fleet: {name} holds {task} in its record, but the threads ledger did not take its claim: {}", error.user_message()),
        }
        Ok(())
    }

    /// Clears every claim an exited session left in the ledger, whichever way it exited, so dispatch, launch and `threads show` stop naming it; a parked, idle or working session keeps its claims. Each owner is swept once and marked, so a later sweep asks the ledger only about sessions that exited since, in reads of at most [`OWNERS_PER_READ`] owners; a read that fails keeps the marks of the reads before it. Each read asks only about sessions still exited when it starts, and marks only those still exited when it ends, so a session revived during a long sweep keeps its claims and is swept when it exits again. Returns how many it cleared.
    pub fn release_exited_claims(&self) -> NorthResult<usize> {
        let unswept = self.unswept()?;
        let mut released = 0;
        for chunk in unswept.chunks(OWNERS_PER_READ) {
            let exited = self.exited()?;
            let owners: BTreeSet<String> = chunk.iter().filter(|name| exited.contains(*name)).cloned().collect();
            if owners.is_empty() {
                continue;
            }
            let swept = claims::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
            released += swept.released;
            let exited = self.exited()?;
            for owner in owners.difference(&swept.unreleased).filter(|owner| exited.contains(*owner)) {
                fs::write(self.root.join(owner).join(CLAIMS_RELEASED), b"")?;
            }
        }
        Ok(released)
    }

    /// Gives up every claim `name` holds in the ledger, the boxes it claimed itself as well as those its launch named, because a forgotten session leaves no directory for the exited sweep to find it by.
    pub(super) fn release_all_claims_of(&self, name: &str) -> NorthResult<()> {
        let owners = BTreeSet::from([name.to_owned()]);
        let swept = claims::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
        if swept.unreleased.is_empty() { Ok(()) } else { Err(NorthError::State(format!("{name} is forgotten but the threads ledger did not release all of its claims"))) }
    }

    /// The exited sessions whose claims no sweep has cleared yet, read from their session directories alone.
    pub fn unswept(&self) -> NorthResult<Vec<String>> {
        Ok(self.exited()?.into_iter().filter(|name| !self.root.join(name).join(CLAIMS_RELEASED).exists()).collect())
    }

    /// The file the background claims sweep holds locked while it runs and writes its outcome to.
    pub fn claims_sweep_log(&self) -> PathBuf {
        self.root.join(CLAIMS_SWEEP_LOG)
    }

    /// Reads who holds each of a launch's `items`, and only those, clearing any claim an exited session left on them; an unread ledger is an error the launch reports and leaves to its claim.
    pub fn release_exited_claims_on(&self, items: &[String]) -> NorthResult<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        Ok(claims::release_exited(self.claims.as_ref(), &self.exited()?, Rows::Items(items))?.released)
    }

    fn exited(&self) -> NorthResult<BTreeSet<String>> {
        Ok(self
            .sessions()?
            .iter()
            .filter(|record| !record.verification && !session_running(record) && self.status(record) == Status::Exited)
            .map(|record| record.name.clone())
            .collect())
    }
}
