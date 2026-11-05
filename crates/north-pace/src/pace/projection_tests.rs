use super::projection::{Reading, project, readings, week_refusal};
use serde_json::json;

fn week(used: f64, burn: Option<f64>, reset_h: f64) -> Reading {
    Reading { window: "week".into(), used_pct: used, burn_pct_h: burn, reset_h: Some(reset_h) }
}

#[test]
fn a_week_the_measured_burn_takes_past_exhaustion_before_its_reset_refuses_the_launch() {
    let burning = [week(90.0, Some(3.0), 20.0)];
    let why = week_refusal(&burning, 10.0, Some(4.0)).unwrap();
    assert_eq!(why, "week projected 102% at 3.0%/h over 240 min before its reset, past 100%");
    assert_eq!(week_refusal(&burning, 10.0, Some(3.0)), None, "nine points over three hours leave the week one");
    assert!(week_refusal(&burning, 8.0, Some(3.0)).is_some(), "this run's earlier launches took two more points");
}

#[test]
fn a_week_that_resets_before_the_eta_or_burns_unmeasured_carries_the_launch() {
    assert_eq!(week_refusal(&[week(90.0, Some(3.0), 2.0)], 10.0, Some(4.0)), None, "two hours at 3%/h reach 96% and then the week resets");
    assert_eq!(week_refusal(&[week(99.0, None, 20.0)], 1.0, Some(4.0)), None);
    assert_eq!(week_refusal(&[week(90.0, Some(3.0), 20.0)], 10.0, None), None);
}

#[test]
fn a_launch_records_each_burning_window_at_its_eta_and_marks_one_that_resets_first() {
    let control = json!({"windows": [
        {"window": "5h", "used_pct": 40.0, "burn_pct_h": 6.0, "reset_h": 0.5},
        {"window": "week", "used_pct": 70.0, "burn_pct_h": 1.5, "reset_h": 30.0},
        {"window": "opus", "used_pct": 10.0, "burn_pct_h": 1.0, "reset_h": 30.0},
    ]});
    let projection = project(&readings(&control), Some(2.0)).unwrap();
    assert_eq!(projection.eta_h, 2.0);
    let shown: Vec<(&str, f64, bool)> = projection.windows.iter().map(|w| (w.window.as_str(), w.projected_pct, w.at_eta)).collect();
    assert_eq!(shown, [("5h", 43.0, false), ("week", 73.0, true)]);
    assert_eq!(project(&readings(&control), None), None);
}
