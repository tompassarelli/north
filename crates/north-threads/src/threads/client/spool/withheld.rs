use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

use north_base::lifecycle::Record;

use super::{COMMAND_NOT_FOUND_STATUS, NOT_EXECUTABLE_STATUS, Phase, Spool, SpooledWrite};
use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

pub(super) const WILL_NOT_RUN: &str = "the ledger host will not run `";
/// How long the ledger host's refusal of a verb stands before one of the writes it withheld is sent again to ask.
const PROBE_AFTER: Duration = Duration::from_secs(3600);

pub(super) fn exited(code: i32) -> String {
    format!("the ledger host exited {code}")
}

/// Whether a refusal was the ledger host declining to run the write, which a later host may not, rather than the ledger refusing the row; an exit 126 or 127 says the host could not run it.
fn withheld_by_host(cause: &str) -> bool {
    cause.starts_with(WILL_NOT_RUN) || [NOT_EXECUTABLE_STATUS, COMMAND_NOT_FOUND_STATUS].iter().any(|code| cause == exited(*code))
}

impl Spool {
    /// Queues every write of `verb` the ledger host withheld, since it has just run that verb; returns their names.
    pub(super) fn resend_withheld(&mut self, verb: &str, at: Timestamp) -> NorthResult<Vec<String>> {
        let mut resent = Vec::new();
        for (_, record) in self.withheld()?.into_iter().filter(|(withheld, _)| withheld == verb) {
            self.go(&record.instance, Phase::Queued, &format!("the ledger host ran `{verb}` again, so the write it withheld is sent again"), at)?;
            resent.push(record.instance);
        }
        Ok(resent)
    }

    /// Queues the oldest write of each verb whose every withheld write was refused `PROBE_AFTER` ago or more, to ask whether the host runs it now.
    pub(super) fn probe_withheld(&mut self, at: Timestamp) -> NorthResult<()> {
        let mut verbs: BTreeMap<String, Vec<Record<Phase>>> = BTreeMap::new();
        for (verb, record) in self.withheld()? {
            verbs.entry(verb).or_default().push(record);
        }
        let stood = |record: &Record<Phase>| at.unix_millis() - record.since.unix_millis() >= PROBE_AFTER.as_millis() as i64;
        for (verb, records) in verbs.into_iter().filter(|(_, records)| records.iter().all(stood)) {
            if let Some(oldest) = records.iter().map(|record| record.instance.as_str()).min() {
                self.go(oldest, Phase::Queued, &format!("asking whether the ledger host runs `{verb}` now, an hour or more after it withheld it"), at)?;
            }
        }
        Ok(())
    }

    /// The refused argv writes the ledger host withheld whose files remain, each with its verb.
    fn withheld(&self) -> NorthResult<Vec<(String, Record<Phase>)>> {
        let mut found = Vec::new();
        for record in self.store.instances::<SpooledWrite>()? {
            if record.state != Phase::Refused || !record.instance.ends_with(".argv") || !withheld_by_host(&record.cause) {
                continue;
            }
            let Ok(text) = fs::read_to_string(self.directory.join(&record.instance)) else { continue };
            let Some(verb) = serde_json::from_str::<Vec<String>>(&text).ok().and_then(|argv| argv.into_iter().next()) else { continue };
            found.push((verb, record));
        }
        Ok(found)
    }
}
