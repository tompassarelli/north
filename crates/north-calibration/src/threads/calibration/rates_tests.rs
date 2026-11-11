use super::forward::Figure;
use super::rates::{Cost, Sample, measure};

fn sample(at: &str, used_pct: f64, resets_at: &str) -> Sample {
    Sample { account: "a".into(), window: "week", at: at.into(), used_pct, resets_at: resets_at.into() }
}

fn points_per_usd(samples: &[Sample], costs: &[Cost]) -> f64 {
    let rows = measure(samples, costs);
    match rows.iter().find(|row| row.cell == "a week").and_then(|row| row.value) {
        Some(Figure::Real(value)) => value,
        other => panic!("no a week rate in {other:?}"),
    }
}

#[test]
fn a_week_rate_is_points_per_api_dollar_with_a_run_spread_over_the_hours_it_ran_and_a_reset_read_a_second_apart() {
    let samples = [
        sample("2026-11-07T10:05:00Z", 10.0, "2026-11-12T02:00:00Z"),
        sample("2026-11-07T10:55:00Z", 12.0, "2026-11-12T01:59:59Z"),
        sample("2026-11-07T11:55:00Z", 14.0, "2026-11-12T02:00:00Z"),
    ];
    let costs = [Cost { account: "a".into(), started: "2026-11-07T10:30:00Z".into(), ended: "2026-11-07T11:30:00Z".into(), usd: 4.0 }];
    assert!((points_per_usd(&samples, &costs) - 1.0).abs() < 1e-9, "4 dollars over two half hours against rises of 2 and 2 points");
}

/// The 2026-11-08 2:05 AM recalibration on b: a per-account row from an earlier calibration must not outrank the newest one.
#[test]
fn an_account_row_from_an_earlier_calibration_yields_to_the_newest_calibrations_pooled_rate() {
    let db = rusqlite::Connection::open_in_memory().expect("memory db");
    db.execute_batch(&format!(
        "CREATE TABLE calibration(at TEXT, kind TEXT, cell TEXT, n INTEGER, value REAL, source TEXT DEFAULT '{}');
         INSERT INTO calibration(at, kind, cell, n, value) VALUES ('2026-11-07T18:15:46Z', 'rate', 'b five_hour', 40, 7.0), ('2026-11-07T18:15:46Z', 'rate', 'b week', 40, 0.4),
                                        ('2026-11-07T18:15:46Z', 'rate', 'pooled five_hour', 99, 3.529), ('2026-11-07T18:15:46Z', 'rate', 'pooled week', 99, 0.3),
                                        ('2026-11-07T19:05:21Z', 'rate', 'pooled five_hour', 135, 0.465), ('2026-11-07T19:05:21Z', 'rate', 'pooled week', 135, 0.2);",
        super::rates::method_stamp()
    ))
    .expect("rows");
    let now = crate::foundation::clock::Timestamp::parse("2026-11-07T19:10:00Z").expect("instant");
    let super::rates::Priced::Measured(rates) = super::rates::priced(&db, "b", now).expect("read") else { panic!("no rates") };
    assert_eq!(rates.five_hour.points_per_usd(), 0.465);
    assert_eq!(rates.week.points_per_usd(), 0.2);
}

#[test]
fn the_rate_that_froze_b_on_at_1_55_am_on_2026_11_08_was_measured_before_the_rate_fix_and_prices_no_spend() {
    use super::rates::{Priced, method_stamp, priced};
    use crate::foundation::clock::Timestamp;
    let db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE calibration(at TEXT, kind TEXT, cell TEXT, n INTEGER, value REAL, source TEXT)").unwrap();
    let row = |at: &str, cell: &str, n: i64, value: f64, source: &str| db.execute("INSERT INTO calibration VALUES (?1, 'rate', ?2, ?3, ?4, ?5)", rusqlite::params![at, cell, n, value, source]).unwrap();
    row("2026-11-07T18:15:46Z", "pooled five_hour", 99, 3.529, "measured n=99 hours 7d");
    row("2026-11-07T18:15:46Z", "pooled week", 175, 1.618, "measured n=175 hours 7d");
    let trip = Timestamp::parse("2026-11-07T18:55:16Z").unwrap();
    match priced(&db, "b", trip).unwrap() {
        Priced::Stale(why) => assert!(why.contains("pooled five_hour") && why.contains("2026-11-07T18:15:46Z") && why.contains("earlier rate method"), "{why}"),
        other => panic!("the pre-fix row priced spend: {other:?}"),
    }
    let stamped = format!("measured n=133 hours 7d {}", method_stamp());
    row("2026-11-07T20:00:58Z", "pooled five_hour", 133, 0.465, &stamped);
    row("2026-11-07T20:00:58Z", "pooled week", 208, 0.280, &stamped);
    assert!(matches!(priced(&db, "b", Timestamp::parse("2026-11-07T20:30:00Z").unwrap()).unwrap(), Priced::Measured(_)));
    match priced(&db, "b", Timestamp::parse("2026-11-08T00:00:00Z").unwrap()).unwrap() {
        Priced::Stale(why) => assert!(why.contains("spend_rate_max_h"), "{why}"),
        other => panic!("a row four hours old priced spend: {other:?}"),
    }
}
