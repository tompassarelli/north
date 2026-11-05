use std::collections::BTreeSet;
use std::fs;

use crate::error::NorthResult;
use crate::fleet::claims::{self, Rows};
use crate::fleet::model::{Account, LaunchRequest, SessionRecord, Status};
use crate::fleet::records::CLAIMS_RELEASED;
use crate::fleet::runtime::Opening;

use super::{Fleet, session_running};

/// How many owners one `threads holder` read names, so a fleet with hundreds of exited sessions never asks for them all in a read the ledger cannot answer within its bound.
const OWNERS_PER_READ: usize = 64;

/// How many owner reads one verb spends on the sweep; a larger backlog drains over the verbs that follow.
const SWEEP_READS: usize = 2;

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
    /// Clears every claim an exited session left in the ledger, whichever way it exited, so dispatch, launch and `threads show` stop naming it; a parked, idle or working session keeps its claims. Each owner is swept once and marked, so later verbs ask the ledger only about sessions that exited since; one call reads at most [`SWEEP_READS`] chunks of [`OWNERS_PER_READ`] owners. Returns how many it cleared.
    pub fn release_exited_claims(&self) -> NorthResult<usize> {
        let unswept: Vec<String> = self.exited()?.into_iter().filter(|name| !self.root.join(name).join(CLAIMS_RELEASED).exists()).collect();
        let mut released = 0;
        for chunk in unswept.chunks(OWNERS_PER_READ).take(SWEEP_READS) {
            let owners: BTreeSet<String> = chunk.iter().cloned().collect();
            let swept = claims::release_exited(self.claims.as_ref(), &owners, Rows::Owners(&owners))?;
            released += swept.released;
            for owner in owners.difference(&swept.unreleased) {
                fs::write(self.root.join(owner).join(CLAIMS_RELEASED), b"")?;
            }
        }
        Ok(released)
    }

    /// Reads who holds each of a launch's `items`, and only those, clearing any claim an exited session left on them; an unread ledger is an error before the launch makes anything.
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
