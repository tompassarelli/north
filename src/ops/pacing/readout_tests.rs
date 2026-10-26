use super::readout;
use rusqlite::Connection;
use serde_json::json;

fn reading(index: usize) -> serde_json::Value {
    let account = if index.is_multiple_of(2) { "a" } else { "b" };
    json!({"ts": format!("2026-10-26T05:{:02}:00Z", index % 60), "provider": "claude", "account": account, "kind": "window", "name": "session", "used_pct": index})
}

#[test]
fn a_file_readout_is_read_in_place_and_only_a_memory_readout_is_copied() {
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("threads.db");
    let db = Connection::open(&file).unwrap();
    readout::prepare(&db).unwrap();
    assert_eq!(readout::readable_path(&db, scratch.path()).unwrap(), file.to_string_lossy());
    assert!(!scratch.path().join("readout.db").exists());
    let memory = Connection::open_in_memory().unwrap();
    readout::prepare(&memory).unwrap();
    readout::record(&memory, &[reading(0)]).unwrap();
    let copy = readout::readable_path(&memory, scratch.path()).unwrap();
    let rows: i64 = Connection::open(copy).unwrap().query_row("SELECT count(*) FROM usage", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn each_prepared_readout_carries_usage_planner_stats_for_its_current_size() {
    let db = Connection::open_in_memory().unwrap();
    readout::prepare(&db).unwrap();
    readout::record(&db, &(0..40).map(reading).collect::<Vec<_>>()).unwrap();
    readout::prepare(&db).unwrap();
    let stat: String = db.query_row("SELECT stat FROM sqlite_stat1 WHERE tbl = 'usage' AND idx = 'usage_latest'", [], |r| r.get(0)).unwrap();
    assert_eq!(stat.split(' ').next(), Some("40"));
}

#[test]
fn recording_the_kept_history_again_each_minute_adds_only_the_new_readings() {
    let db = Connection::open_in_memory().unwrap();
    readout::prepare(&db).unwrap();
    let history: Vec<_> = (0..30).map(reading).collect();
    readout::record(&db, &history).unwrap();
    let next: Vec<_> = (0..31).map(reading).collect();
    readout::record(&db, &next).unwrap();
    readout::record(&db, &next).unwrap();
    let rows: i64 = db.query_row("SELECT count(*) FROM usage", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 31);
}
