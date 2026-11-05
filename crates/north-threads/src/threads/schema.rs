//! The NorthThreads ledger's own tables, created before any verb reads or writes a ledger this host holds.
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

use crate::threads::hold;
use crate::threads::verb::{Failure, immediate};

const SCHEMA: &str = include_str!("ledger.sql");
const RETRIES: u32 = 6000;
const RETRY_PAUSE: std::time::Duration = std::time::Duration::from_millis(10);

/// Creates the ledger at `path` with every NorthThreads table it lacks, in WAL mode. Switching a new file to WAL
/// answers busy at once, without the busy handler, when another process is creating it too, so a busy answer is
/// retried until the ingest hold cap or a minute has passed.
pub fn ensure(path: &Path) -> Result<(), Failure> {
    let mut tries = 0;
    loop {
        match attempt(path) {
            Err(Failure::Refused(why)) if why.contains("database is locked") && tries < RETRIES && !hold::past_deadline() => {
                tries += 1;
                std::thread::sleep(RETRY_PAUSE);
            }
            other => return other,
        }
    }
}

fn attempt(path: &Path) -> Result<(), Failure> {
    let failed = |error: rusqlite::Error| Failure::Refused(format!("ledger {}: {error}", path.display()));
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|error| Failure::Refused(format!("ledger {}: {error}", parent.display())))?;
    }
    let mut db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(failed)?;
    crate::threads::hold::wait_when_busy(&db).map_err(failed)?;
    let mode: String = db.query_row("PRAGMA journal_mode", [], |row| row.get(0)).map_err(failed)?;
    if mode != "wal" {
        db.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(())).map_err(failed)?;
    }
    let tx = immediate(&mut db)?;
    tx.execute_batch(SCHEMA)?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure;

    #[test]
    fn processes_creating_the_same_new_ledger_at_once_all_succeed() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("state/threads.db");
        let outcomes: Vec<bool> = std::thread::scope(|scope| {
            let creators: Vec<_> = (0..8).map(|_| scope.spawn(|| ensure(&path).is_ok())).collect();
            creators.into_iter().map(|creator| creator.join().unwrap()).collect()
        });
        assert_eq!(outcomes, [true; 8]);
    }

    #[test]
    fn a_new_ledger_gets_every_table_in_wal_mode_and_a_second_ensure_changes_nothing() {
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("state/threads.db");
        ensure(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("INSERT INTO claims(item, owner, since) VALUES ('wisp#7', 'fixture', '2026-11-05T00:00:00Z')", []).unwrap();
        drop(db);
        ensure(&path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let tables: Vec<String> = db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(tables, ["blocks", "calibration", "claims", "events", "gh_cache", "item_needs", "items", "job_queue", "job_runs", "run_costs", "run_events", "run_usage", "runs", "tier_choice"]);
        let mode: String = db.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
        let kept: i64 = db.query_row("SELECT count(*) FROM claims", [], |row| row.get(0)).unwrap();
        assert_eq!((mode.as_str(), kept), ("wal", 1));
        let runs: Vec<String> = db.prepare("SELECT name FROM pragma_table_info('runs')").unwrap().query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
        assert!(["provider", "brief_sha256", "tool_calls", "active_min", "wait_min"].iter().all(|column| runs.iter().any(|name| name == column)), "{runs:?}");
    }
}
