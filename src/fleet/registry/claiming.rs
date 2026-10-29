use crate::error::NorthResult;
use crate::fleet::claims;
use crate::fleet::model::{Account, LaunchRequest, SessionRecord};
use crate::fleet::runtime::Opening;

use super::Fleet;

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
    pub(super) fn start_claiming(&self, record: &mut SessionRecord, account: &Account, conversation: Option<&str>) -> NorthResult<()> {
        let started = self.start(record, account, Opening::Fresh(conversation));
        if started.is_err() {
            claims::release_for(self.claims.as_ref(), record);
        }
        started
    }
}
