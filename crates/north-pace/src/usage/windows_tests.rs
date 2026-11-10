use rusqlite::Connection;

use super::decide::world::{ModelFamily, Window};

/// Opened by north#1065: the capacity read model reads the same Fable meter the account view shows. With no scoped
/// Fable window the meter is the shared week's, and a reported scoped window keeps its own reading; a full week
/// with a reset still ahead is also a limit event.
#[test]
fn the_windows_read_carries_each_accounts_fable_meter_and_its_limit_events() {
    let db = Connection::open_in_memory().unwrap();
    db.execute("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT", []).unwrap();
    for (account, name, used, resets_at) in [("a", "weekly_all", 40.0, Some("2026-11-03T00:00:00Z")), ("b", "weekly_all", 100.0, Some("2026-11-03T00:00:00Z")), ("b", "weekly_scoped/Fable", 3.0, None)] {
        db.execute("INSERT INTO usage VALUES ('2026-10-28T11:50:00Z', 'claude', ?1, 'window', ?2, ?3, 10080, ?4, NULL, 'header')", rusqlite::params![account, name, used, resets_at]).unwrap();
    }
    let read = super::windows(&db, 1_793_000_000_000);
    let fable = |account: &str| read.readings.iter().find(|reading| reading.account.0 == account && reading.window == Window::Scoped(ModelFamily::Fable)).map(|reading| reading.used.0);
    assert_eq!((fable("a"), fable("b")), (Some(40.0), Some(3.0)));
    let limited: Vec<&str> = read.limits.iter().map(|event| event.account.0.as_str()).collect();
    assert_eq!(limited, ["b"]);
}
