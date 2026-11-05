use std::collections::BTreeSet;

use crate::error::NorthResult;
use crate::fleet::claims::{self, Rows};
use crate::fleet::model::{Account, LaunchRequest, SessionRecord, Status};
use crate::fleet::runtime::Opening;

use super::{Fleet, session_running};

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
    /// Clears every claim an exited session left in the ledger, whichever way it exited, so dispatch, launch and `threads show` stop naming it; a parked, idle or working session keeps its claims. Returns how many it cleared.
    pub fn release_exited_claims(&self) -> NorthResult<usize> {
        let exited = self.exited()?;
        if exited.is_empty() {
            return Ok(0);
        }
        claims::release_exited(self.claims.as_ref(), &exited, Rows::Owners(&exited))
    }

    /// Reads who holds each of a launch's `items`, and only those, clearing any claim an exited session left on them; an unread ledger is an error before the launch makes anything.
    pub fn release_exited_claims_on(&self, items: &[String]) -> NorthResult<usize> {
        if items.is_empty() {
            return Ok(0);
        }
        claims::release_exited(self.claims.as_ref(), &self.exited()?, Rows::Items(items))
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
