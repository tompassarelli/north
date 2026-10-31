use std::path::PathBuf;

use crate::error::NorthResult;

use super::Fleet;
use super::super::model::SessionRecord;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Succession {
    Refuse,
    TakeOver,
}

/// The session a takeover ended, and where its directory now lies.
pub(super) struct Predecessor {
    record: SessionRecord,
    pub(super) archived: PathBuf,
}

impl Predecessor {
    /// The successor takes every message to the name from where the predecessor's line began, so a report sent before the takeover that the old session never took still arrives.
    pub(super) fn addressed_since(&self) -> u64 {
        self.record.addressed_since()
    }
}

impl Fleet {
    /// Ends and archives the session holding `name` when this launch takes the name over; None when no session holds it.
    pub(super) fn retire_for(&self, name: &str, succession: Succession) -> NorthResult<Option<Predecessor>> {
        if succession != Succession::TakeOver || !self.session_dir(name)?.exists() {
            return Ok(None);
        }
        let record = self.record(name)?;
        let archived = self.archive(name)?;
        eprintln!("north fleet: {name} was held by conversation {}; it is stopped and archived at {}, and the name and its messages now go to the new session", record.conversation.as_deref().unwrap_or("none"), archived.display());
        Ok(Some(Predecessor { record, archived }))
    }
}
