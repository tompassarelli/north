use rusqlite::Connection;

use super::ensure;

const OLD_TABLE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";

fn put(db: &Connection, ts: &str, account: Option<&str>, source: Option<&str>, used: f64) {
    db.execute("INSERT INTO usage(ts, provider, account, kind, name, used_pct, source) VALUES (?, 'claude', ?, 'window', 'week', ?, ?)", rusqlite::params![ts, account, used, source]).unwrap();
}

fn count(db: &Connection) -> i64 {
    db.query_row("SELECT count(*) FROM usage", [], |row| row.get(0)).unwrap()
}

#[test]
fn a_ledger_holding_repeated_readings_keeps_the_first_of_each_and_then_refuses_repeats() {
    let db = Connection::open_in_memory().unwrap();
    db.execute(OLD_TABLE, []).unwrap();
    for used in [1.0, 2.0, 3.0] {
        put(&db, "2026-10-29T10:00:00Z", Some("a"), Some("web"), used);
        put(&db, "2026-10-29T10:00:00Z", None, None, used);
    }
    put(&db, "2026-10-29T10:00:00Z", Some("a"), Some("cli"), 9.0);
    put(&db, "2026-10-29T10:01:00Z", Some("a"), Some("web"), 4.0);
    assert_eq!(count(&db), 8);

    ensure(&db).unwrap();

    assert_eq!(count(&db), 4);
    let kept: f64 = db.query_row("SELECT used_pct FROM usage WHERE account = 'a' AND source = 'web' AND ts LIKE '%10:00:00Z'", [], |row| row.get(0)).unwrap();
    assert_eq!(kept, 1.0);
    let ignore = "INSERT OR IGNORE INTO usage(ts, provider, account, kind, name, source) VALUES ('2026-10-29T10:00:00Z', 'claude', ?, 'window', 'week', ?)";
    db.execute(ignore, rusqlite::params![None::<&str>, None::<&str>]).unwrap();
    db.execute(ignore, rusqlite::params!["a", "web"]).unwrap();
    assert_eq!(count(&db), 4);
    let plain = db.execute("INSERT INTO usage(ts, provider, account, kind, name, source) VALUES ('2026-10-29T10:00:00Z', 'claude', NULL, 'window', 'week', NULL)", []);
    assert!(plain.is_err());
}

#[test]
fn ensuring_an_already_keyed_ledger_changes_nothing() {
    let db = Connection::open_in_memory().unwrap();
    ensure(&db).unwrap();
    put(&db, "2026-10-29T10:00:00Z", Some("a"), Some("web"), 1.0);
    ensure(&db).unwrap();
    assert_eq!(count(&db), 1);
}
