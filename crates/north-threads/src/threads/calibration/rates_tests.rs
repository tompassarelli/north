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
