use super::projections::{Ledger, Readings, Scored, errors, lines, scored};
use super::tests::{Issues, ledger};
use super::{Prior, tick};
use crate::foundation::clock::Timestamp;
use crate::dispatch::LaunchRecord;
use crate::pace::projection::{Projected, Projection};
use rusqlite::Connection;

const NOW: f64 = 1_800_000_000.0;
const HOUR: f64 = 3600.0;

fn window(name: &str, projected: f64, at_eta: bool) -> Projected {
    Projected { window: name.into(), used_pct: projected - 4.0, projected_pct: projected, horizon_h: 1.0, at_eta }
}

fn launch(at: f64, launched: bool, eta_h: f64, windows: Vec<Projected>) -> LaunchRecord {
    LaunchRecord {
        at,
        item: "north#255".into(),
        host: "gex45".into(),
        account: "claude:a".into(),
        name: "north-255-d1200".into(),
        cloud: false,
        launched,
        detail: String::new(),
        tier: None,
        category: None,
        arm: false,
        handoff: None,
        hand: false,
        effort_arm: None,
        projection: Some(Projection { eta_h, windows }),
    }
}

struct Off(f64);

impl Readings for Off {
    fn at(&self, account: &str, window: &str, at: f64) -> Option<f64> {
        assert_eq!(account, "claude:a");
        let step = ((NOW - at) / HOUR).round();
        Some(if window == "5h" { 50.0 - self.0 * (step % 4.0) } else { 60.0 + step % 3.0 })
    }
}

fn week_of_launches() -> Vec<LaunchRecord> {
    (1..=120).map(|step| launch(NOW - f64::from(step) * HOUR - HOUR, true, 1.0, vec![window("5h", 50.0, true), window("week", 60.0, true)])).collect()
}

#[test]
fn tune_prints_the_p50_and_p90_projection_error_of_both_windows_over_a_week_of_launches() {
    let found = scored(&week_of_launches(), NOW - 7.0 * 24.0 * HOUR, NOW, &Off(2.0));
    assert_eq!(found.len(), 240);
    let error = errors(&found);
    assert_eq!((error[0].window.as_str(), error[0].n, error[0].p50, error[0].p90), ("5h", 120, 2.0, 6.0));
    assert_eq!((error[1].window.as_str(), error[1].n, error[1].p50, error[1].p90), ("week", 120, 1.0, 2.0));
    let ticked = tick(&super::Ledger { projections: Ok(found), ..ledger() }, &Prior::default(), &[], &mut Issues::default(), "2027-01-15T08:00:00Z", true);
    for line in ["projection 5h at ETA against actual: error p50 2.0 p90 6.0 points, bias +3.0 (n 120)", "projection week at ETA against actual: error p50 1.0 p90 2.0 points, bias -1.0 (n 120)"] {
        assert!(ticked.lines.contains(&line.to_owned()), "{line}: {:#?}", ticked.lines);
    }
}

#[test]
fn only_a_started_launch_of_the_week_whose_eta_came_before_its_window_reset_is_scored() {
    let since = NOW - 7.0 * 24.0 * HOUR;
    let launches = [
        launch(NOW - 3.0 * HOUR, false, 1.0, vec![window("5h", 50.0, true)]),
        launch(NOW - 0.5 * HOUR, true, 1.0, vec![window("5h", 50.0, true)]),
        launch(since - HOUR, true, 1.0, vec![window("5h", 50.0, true)]),
        launch(NOW - 3.0 * HOUR, true, 1.0, vec![window("5h", 50.0, false), window("week", 61.0, true)]),
    ];
    assert_eq!(scored(&launches, since, NOW, &Off(2.0)), [Scored { window: "week".into(), projected: 61.0, actual: 62.0 }]);
}

#[test]
fn a_tick_with_no_scored_launch_says_so_and_one_that_cannot_read_the_usage_ledger_fails() {
    assert_eq!(lines(&[]), ["projection 5h at ETA against actual: no launch scored", "projection week at ETA against actual: no launch scored"]);
    let unread = tick(&super::Ledger { projections: Err("no usage table".into()), ..ledger() }, &Prior::default(), &[], &mut Issues::default(), "2027-01-15T08:00:00Z", true);
    assert!(unread.failed && unread.lines.contains(&"projections: unread (no usage table)".to_owned()), "{:#?}", unread.lines);
}

#[test]
fn the_usage_ledger_gives_each_window_its_first_reading_at_or_after_the_eta() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT").unwrap();
    let stamp = |at: f64| Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset();
    let rows = [(NOW - 60.0, "five_hour", 300, 40.0), (NOW + 120.0, "five_hour", 300, 47.0), (NOW + 300.0, "five_hour", 300, 49.0), (NOW + 60.0, "weekly_all", 10080, 71.0), (NOW + 60.0, "five_hour", 300, 99.0)];
    for (index, (at, name, minutes, used)) in rows.into_iter().enumerate() {
        let account = if index == 4 { "b" } else { "a" };
        db.execute("INSERT INTO usage VALUES(?1, 'claude', ?2, 'window', ?3, ?4, ?5, '', NULL, 'oauth')", rusqlite::params![stamp(at), account, name, used, minutes]).unwrap();
    }
    let ledger = Ledger(db);
    assert_eq!(ledger.at("claude:a", "5h", NOW), Some(47.0));
    assert_eq!(ledger.at("claude:a", "week", NOW), Some(71.0));
    assert_eq!(ledger.at("claude:a", "5h", NOW + 2.0 * HOUR), None, "no reading within the slack after the ETA");
}
