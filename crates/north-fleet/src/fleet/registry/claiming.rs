use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::error::NorthResult;
use crate::fleet::claims::{self, Rows};
use crate::fleet::model::{Account, LaunchRequest, SessionRecord, Status};
use crate::fleet::records::{CLAIMS_RELEASED, CLAIMS_SWEEP_LOG};
use crate::fleet::runtime::Opening;

use super::{Fleet, session_running};

/// How many owners one `threads holder` read names, so a fleet with hundreds of exited sessions never asks for them all in a read the ledger cannot answer within its bound.
const OWNERS_PER_READ: usize = 64;

impl Fleet {
    /// Claims every task of a launch before its session starts: a live holder refuses it unless `--force-dup` put it beside the holder. A `--test` session claims nothing.
    pub(super) fn claim_before_start(&self, request: &LaunchRequest, tasks: &[String], verification: bool) -> NorthResult<()> {
        if verification {
            return Ok(());
        }
        let beside_holder = request.placement.as_deref().is_some_and(|note| note.contains("dup: "));
        match claims::hold_all(self.claims.as_ref(), tasks, &request.name) {
            Err(error) if beside_holder => {
                eprintln!("north fleet: {} runs beside the holder: {}", request.name, error.user_message());
                Ok(())
            }
            result => result,
        }
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
