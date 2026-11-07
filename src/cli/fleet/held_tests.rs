use rusqlite::Connection;

use super::*;

const SINCE: i64 = 1_794_000_000;

fn unix(text: &str) -> u64 {
    u64::try_from(Timestamp::parse(text).unwrap().unix_millis()).unwrap()
}

/// The 2026-11-07 ledger window held 4 capacity and 24 wait stops that nothing woke (north#790): the capacity host's
/// `capacity_priority` row of a run that queued 900 s names the session and gives its queue start and admission, and the
/// ledger's `fleet_wait_history` row of a wait names the session with its registration and settlement, so the resume pass
/// reads both without a word of any report; a run that queued under the bound is no stop.
#[test]
fn the_capacity_and_ledger_rows_of_2026_11_07_read_as_admissions_and_settlements() {
    let capacity = Connection::open_in_memory().unwrap();
    capacity.execute_batch("CREATE TABLE capacity_priority (lease TEXT PRIMARY KEY, at TEXT NOT NULL, owner TEXT NOT NULL, class TEXT NOT NULL, priority TEXT NOT NULL, placed INTEGER NOT NULL, queuedSeconds REAL NOT NULL, command TEXT)").unwrap();
    for (lease, at, owner, queued) in [("a", "2026-11-07T04:30:00.000Z", "gate:north-757-d0001", 900.0), ("b", "2026-11-07T04:31:00.000Z", "gate:north-757-d0002", 20.0)] {
        capacity.execute("INSERT INTO capacity_priority VALUES (?1, ?2, ?3, 'test', 'tom-ask', 1, ?4, NULL)", rusqlite::params![lease, at, owner, queued]).unwrap();
    }
    let ledger = Connection::open_in_memory().unwrap();
    ledger.execute_batch("CREATE TABLE fleet_wait_history(session TEXT, kind TEXT, key TEXT, created TEXT, settled TEXT)").unwrap();
    ledger.execute("INSERT INTO fleet_wait_history VALUES ('north-730-d0001', 'job', 'n730-poolkey-landed', '2026-11-07T05:41:48Z', '2026-11-07T06:03:39Z')", []).unwrap();

    let admitted = admissions(&capacity, SINCE);
    let settled = settlements(&ledger, SINCE);

    assert_eq!(admitted.keys().collect::<Vec<_>>(), ["north-757-d0001"]);
    assert_eq!(admitted["north-757-d0001"], [Admission { queued_ms: unix("2026-11-07T04:15:00Z"), admitted_ms: unix("2026-11-07T04:30:00Z") }]);
    assert_eq!(settled["north-730-d0001"], [Settlement { registered_ms: unix("2026-11-07T05:41:48Z"), settled_ms: unix("2026-11-07T06:03:39Z"), run: "job:n730-poolkey-landed".into() }]);
}
