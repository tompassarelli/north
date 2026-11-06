use rusqlite::{Connection, params_from_iter};

use super::readings_query;
use crate::foundation::clock::Timestamp;
use crate::platform::usage_table;

fn ledger() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    usage_table::ensure(&db).unwrap();
    db.execute_batch(super::SCHEMA).unwrap();
    db
}

fn plan(db: &Connection, account: &str) -> Vec<String> {
    let (sql, bound) = readings_query(Some((account, Some(Timestamp::parse("2026-11-06T10:00:00Z").unwrap()))));
    db.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap().query_map(params_from_iter(bound), |row| row.get(3)).unwrap().collect::<Result<_, _>>().unwrap()
}

#[test]
fn one_account_s_recent_readings_are_searched_by_time_not_scanned_by_provider() {
    let db = ledger();
    let claude = plan(&db, "claude-a");
    assert!(claude.iter().any(|step| step.contains("usage_latest (provider=? AND account=? AND kind=? AND name=? AND ts>?)")), "{claude:?}");
    let gemini = plan(&db, "gemini");
    assert!(gemini.iter().any(|step| step.contains("usage_window_time (provider=? AND kind=? AND name=? AND ts>?)")), "{gemini:?}");
}

#[test]
fn a_touch_reads_from_the_minute_before_it_and_refuses_a_timestamp_with_an_offset() {
    let bound = super::minute_bound(Timestamp::parse("2026-11-06T10:00:30Z").unwrap());
    for kept in ["2026-11-06T10:00:30Z", "2026-11-06T10:00:30.250Z", "2026-11-06T10:01:00Z", "2026-11-07T00:00:00Z"] {
        assert!(kept > bound.as_str(), "{kept} sorts below {bound}");
    }
    assert!("2026-11-06T09:30:00Z" < bound.as_str(), "{bound} reads the half hour before the touch");
    let mut touched = super::Touched::default();
    assert!(touched.add("claude-a".to_owned(), "2026-11-06T10:00:30.250Z").is_ok());
    assert!(touched.add("claude-a".to_owned(), "2026-11-06T12:00:30+02:00").is_err());
}

#[test]
fn a_level_readout_re_prices_without_reading_back_through_its_plateau_and_a_rise_does() {
    let mut db = ledger();
    let read = |db: &Connection, ts: &str, used: f64| {
        db.execute("INSERT INTO usage(ts, provider, account, kind, name, used_pct, resets_at) VALUES (?, 'claude', 'a', 'window', 'weekly_all', ?, '2026-11-12T00:00:00Z')", rusqlite::params![ts, used]).unwrap();
    };
    read(&db, "2026-11-06T10:00:00Z", 50.0);
    read(&db, "2026-11-06T10:20:00Z", 50.0);
    let prices = std::collections::BTreeMap::from([("claude-a".to_owned(), 100.0)]);
    super::allocate(&mut db, &prices, &super::Touched::default()).unwrap();
    read(&db, "2026-11-06T10:10:00Zunreadable", 50.0);
    db.execute("INSERT INTO run_usage(agent, request, ts, account, weight) VALUES ('run', 'request', 'unreadable', 'claude-a', 1.0)", []).unwrap();
    let touch = |ts: &str| {
        let mut touched = super::Touched::default();
        touched.add("claude-a".to_owned(), ts).unwrap();
        touched
    };
    read(&db, "2026-11-06T11:00:00Z", 50.0);
    assert!(super::allocate(&mut db, &prices, &touch("2026-11-06T11:00:00Z")).is_ok());
    read(&db, "2026-11-06T11:01:00Z", 51.0);
    assert!(super::allocate(&mut db, &prices, &touch("2026-11-06T11:01:00Z")).is_err());
}
