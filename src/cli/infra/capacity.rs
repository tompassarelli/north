//! What the capacity ledger says about the last day: per host, the fleet sessions and build jobs it served and the CPU
//! seconds its runs used. An idle paid box is a host the ledger has no row for, which the report marks idle.
use std::collections::BTreeMap;

use north_capacity_host::capacity::host::capacity_database_readonly;
use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

/// The window the report reads, one day, so a box that did nothing since yesterday shows as idle.
pub const WINDOW_SECONDS: i64 = 86_400;

/// One host's rows in the window: the distinct owners it served (a fleet session names itself when capacity serves it),
/// the build jobs it ran, and the CPU seconds they used.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Record {
    pub sessions: usize,
    pub jobs: usize,
    pub cpu_seconds: f64,
}

pub fn read(now: Timestamp) -> Result<BTreeMap<String, Record>, String> {
    let db = capacity_database_readonly().ok_or("capacity ledger: unavailable")?;
    window(&db, now).map_err(|error| format!("capacity ledger: {error}"))
}

fn window(db: &Connection, now: Timestamp) -> rusqlite::Result<BTreeMap<String, Record>> {
    let since = Timestamp::from_unix_seconds(now.unix_seconds() - WINDOW_SECONDS).iso_millis();
    let mut statement = db.prepare(
        "SELECT host, count(DISTINCT owner), count(*), sum(coalesce(cpuSeconds, coalesce(meanCores, 0.0) * coalesce(wallSeconds, 0.0))) \
         FROM capacity_runs WHERE at >= ?1 AND host IS NOT NULL AND host <> '' AND host NOT LIKE 'actions:%' GROUP BY host",
    )?;
    let rows = statement.query_map([since], |row| {
        Ok((
            row.get::<_, String>(0)?,
            Record {
                sessions: usize::try_from(row.get::<_, i64>(1)?).unwrap_or(usize::MAX),
                jobs: usize::try_from(row.get::<_, i64>(2)?).unwrap_or(usize::MAX),
                cpu_seconds: row.get::<_, f64>(3)?,
            },
        ))
    })?;
    rows.collect()
}
