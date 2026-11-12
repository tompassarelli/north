use std::fs;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Phase, SessionRecord};
use crate::fleet::reaper::reported_done;
use crate::fleet::records::write_record;
use crate::fleet::runtime::now_ms;

use super::Fleet;

impl Fleet {
    pub fn revivable(&self, name: &str) -> NorthResult<SessionRecord> {
        self.record(name).or_else(|missing| if self.session_dir(name)?.exists() { Err(missing) } else { self.retired_session(name).map(|(_, record)| record).ok_or(missing) })
    }

    /// The record of `name` as a revive finds it. A session whose launch is retired, archived by a take-over whose new launch never came up or by a forget, resumes in its own conversation when its last report is not a Done: it becomes a new launch of the same record, since a retired launch moves nowhere.
    pub(crate) fn reopened(&self, name: &str) -> NorthResult<SessionRecord> {
        let dir = self.session_dir(name)?;
        if !dir.exists() && let Some((archived, _)) = self.retired_session(name) {
            fs::rename(archived, &dir)?;
        }
        let mut record = self.record(name)?;
        if record.phase() != Some(Phase::Retired) {
            return Ok(record);
        }
        let transcript = self.recent(self.runtime(&record)?.as_ref(), &record)?;
        if reported_done(&transcript.entries) {
            return Err(NorthError::Refused(format!("{name} is retired after its Done report, so it has no unfinished work to resume")));
        }
        record.addressed_since_ms = Some(record.addressed_since());
        record.created_ms = now_ms();
        write_record(&dir, &record)?;
        self.record(name)
    }
}
