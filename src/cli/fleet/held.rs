//! What the fleet tick's resume pass reads, each from the record its owner writes: the lander's queued lanes, the capacity
//! host's runs admitted after a queue and the ledger's settled waits.
use std::collections::BTreeMap;

use rusqlite::{Connection, OpenFlags};

use crate::capacity::host::{capacity_database_readonly, queued_admissions};
use crate::fleet::resume::{Admission, Held, Settlement};
use crate::foundation::clock::Timestamp;
use crate::land_ticket::lanes_by_session;
use crate::platform::{land_queue, records_mirror};
use crate::threads::client;

/// How far back a stop can still be woken; an older one is past waking by itself.
const WITHIN_SECS: i64 = 24 * 3600;

pub(super) fn read(now_secs: i64) -> Held {
    let since = now_secs - WITHIN_SECS;
    Held {
        lanes: lanes_by_session(&land_queue::queues(&records_mirror::state_home())).unwrap_or_default(),
        admissions: capacity_database_readonly().map(|db| admissions(&db, since)).unwrap_or_default(),
        settlements: client::ledger_path().ok().and_then(|path| Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()).map(|db| settlements(&db, since)).unwrap_or_default(),
    }
}

pub(super) fn admissions(db: &Connection, since: i64) -> BTreeMap<String, Vec<Admission>> {
    let ms = |seconds: i64| u64::try_from(seconds.saturating_mul(1000)).unwrap_or_default();
    queued_admissions(db, since)
        .into_iter()
        .map(|(session, runs)| (session, runs.into_iter().map(|run| Admission { queued_ms: ms(run.at).saturating_sub((run.seconds * 1000.0) as u64), admitted_ms: ms(run.at) }).collect()))
        .collect()
}

pub(super) fn settlements(db: &Connection, since: i64) -> BTreeMap<String, Vec<Settlement>> {
    let mut settled: BTreeMap<String, Vec<Settlement>> = BTreeMap::new();
    let Ok(mut statement) = db.prepare("SELECT session, kind, key, created, settled FROM fleet_wait_history WHERE settled >= ?1") else { return settled };
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)));
    let ms = |text: &str| Timestamp::parse(text).ok().and_then(|at| u64::try_from(at.unix_millis()).ok());
    for (session, kind, key, created, ended) in rows.into_iter().flatten().filter_map(Result::ok) {
        if let (Some(registered_ms), Some(settled_ms)) = (ms(&created), ms(&ended)) {
            settled.entry(session).or_default().push(Settlement { registered_ms, settled_ms, run: format!("{kind}:{key}") });
        }
    }
    settled
}

#[cfg(test)]
#[path = "held_tests.rs"]
mod tests;
