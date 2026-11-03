use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use super::Verdict;
use crate::threads::calibration::rework::{Rework, Sources, alarm_pct};

const WEEK_S: i64 = 7 * 86_400;

/// The 7-day rework rate line per repository, and a failure naming each repository above `alarm` percent.
pub(super) fn verdict(rates: &BTreeMap<String, (usize, usize)>, alarm: f64) -> (String, Verdict) {
    let pct = |(reworked, landed): (usize, usize)| reworked as f64 * 100.0 / landed.max(1) as f64;
    let shown: Vec<String> = rates.iter().map(|(repo, rate)| format!("{repo} {}/{} {:.0}%", rate.0, rate.1, pct(*rate))).collect();
    let line = if shown.is_empty() { "no landings on main in 7 days".to_owned() } else { shown.join(", ") };
    let over: Vec<String> = rates.iter().filter(|(_, rate)| pct(**rate) > alarm).map(|(repo, rate)| format!("{repo} {:.0}%", pct(*rate))).collect();
    let result = if over.is_empty() { Ok(()) } else { Err(format!("7-day rework rate over {alarm:.0}%: {}", over.join(", "))) };
    (line, result)
}

pub(super) fn live(db: &Path, now: u64) -> (String, Verdict) {
    let read = || -> rusqlite::Result<BTreeMap<String, (usize, usize)>> {
        let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Rework::read(&connection, &Sources::live())?.rates(now as i64 - WEEK_S))
    };
    match read() {
        Ok(rates) => verdict(&rates, alarm_pct()),
        Err(error) => (String::new(), Err(format!("{}: {error}", db.display()))),
    }
}
