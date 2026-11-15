use crate::foundation::clock::Timestamp;
use north_base::foundation::exit::NO_VERDICT_KIND;
use north_base::land_ticket::Phase;
use north_base::lifecycle::State;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

/// The land tickets refused in a span, split by whether the refusing step reached a verdict: the lander names the log of a step whose program exited `NO_VERDICT` `NAME.KIND.no-verdict.log`, as a regenerate step does that finds the runtime it runs on not yet whole and no build-ahead artifact for its tree (north#1570).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LandRefusals {
    pub(super) decided: usize,
    pub(super) no_verdict: usize,
}

pub(super) fn land_refusals(ledger: &Path, (start, end): (Timestamp, Timestamp)) -> LandRefusals {
    let Ok(db) = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return LandRefusals::default() };
    let Ok(mut statement) = db.prepare("SELECT cause FROM lifecycle_moves WHERE machine = 'land-ticket' AND to_state = ?3 AND refused = 0 AND at_ms >= ?1 AND at_ms < ?2") else { return LandRefusals::default() };
    let causes: Vec<String> = statement.query_map(rusqlite::params![start.unix_millis(), end.unix_millis(), Phase::Failed.name()], |row| row.get(0)).map(|rows| rows.filter_map(Result::ok).collect()).unwrap_or_default();
    let suffix = format!(".{NO_VERDICT_KIND}.log");
    let no_verdict = causes.iter().filter(|cause| cause.ends_with(&suffix)).count();
    LandRefusals { decided: causes.len() - no_verdict, no_verdict }
}
