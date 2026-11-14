//! What the fleet's books say: its spend, its gates, and the landings it has observed.

use crate::error::NorthResult;
use crate::fleet::books::{Digest, Revision, SessionSpend};
use crate::fleet::gates;
use crate::fleet::landings::{self, push_log};

use super::Fleet;

impl Fleet {
    pub fn observe_landings(&self) -> NorthResult<()> {
        let mut log = landings::PushLog::read(&push_log());
        for record in self.sessions()? {
            landings::observe_among(self.ledger.as_ref(), &record, &log.of(&record))?;
        }
        Ok(())
    }

    pub fn spend_of(&self, name: &str) -> NorthResult<Option<SessionSpend>> {
        Ok(self.spend()?.into_iter().find(|spend| spend.session == name))
    }

    pub fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        self.ledger.spend()
    }

    pub fn books_revision(&self) -> NorthResult<Option<Revision>> {
        self.ledger.revision()
    }

    pub fn digest(&self, since_ms: u64) -> NorthResult<Digest> {
        Ok(Digest { gates: gates::decisions(self.ledger.gates(since_ms)?), spend: self.ledger.spend()?, landed: self.ledger.landed_since(since_ms)? })
    }
}
