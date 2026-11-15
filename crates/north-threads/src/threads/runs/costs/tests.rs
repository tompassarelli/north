use rusqlite::Connection;

use north_base::platform::usage_table;

fn ledger() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(crate::threads::fixtures::LEDGER_SCHEMA).unwrap();
    usage_table::ensure(&db).unwrap();
    db.execute_batch(super::SCHEMA).unwrap();
    super::super::columns::ensure(&db).unwrap();
    db
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
    for (agent, model, session_usd) in [("free", super::FREE_MODEL, None), ("paid", "opencode-go/deepseek-v4.1-flash", None), ("held", "opencode-go/deepseek-v4.1-flash", Some(0.25))] {
        db.execute("INSERT INTO runs(agent, tier, category, ended, minutes, outcome, model, session_usd) VALUES (?, 'worker', 'unknown', '2026-11-06T10:30:00Z', 1, 'done', ?, ?)", rusqlite::params![agent, model, session_usd]).unwrap();
    }
    super::allocate(&mut db, &prices, &super::Touched::default()).unwrap();
    let cost = |agent: &str| db.query_row("SELECT cost_usd FROM runs WHERE agent = ?", [agent], |row| row.get::<_, Option<f64>>(0)).unwrap();
    assert_eq!((cost("free"), cost("paid"), cost("held")), (Some(0.0), None, Some(0.25)), "a Go run whose session reported its cost keeps it through the re-price");
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
