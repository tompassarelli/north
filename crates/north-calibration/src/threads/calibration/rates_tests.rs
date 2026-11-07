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
    db.execute_batch(
        "CREATE TABLE calibration(at TEXT, kind TEXT, cell TEXT, n INTEGER, value REAL);
         INSERT INTO calibration VALUES ('2026-11-07T18:15:46Z', 'rate', 'b five_hour', 40, 7.0), ('2026-11-07T18:15:46Z', 'rate', 'b week', 40, 0.4),
                                        ('2026-11-07T18:15:46Z', 'rate', 'pooled five_hour', 99, 3.529), ('2026-11-07T18:15:46Z', 'rate', 'pooled week', 99, 0.3),
                                        ('2026-11-07T19:05:21Z', 'rate', 'pooled five_hour', 135, 0.465), ('2026-11-07T19:05:21Z', 'rate', 'pooled week', 135, 0.2);",
    )
    .expect("rows");
    let rates = super::rates::rates_of(&db, "b").expect("read").expect("rates");
    assert_eq!(rates.five_hour.points_per_usd(), 0.465);
    assert_eq!(rates.week.points_per_usd(), 0.2);
}
