use serde_json::{Value, json};

use super::{State, calibration, jsonl, render};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/ops/report/fixtures");

fn fixture(name: &str) -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()).unwrap()
}

#[test]
fn test_report_renders_the_fixture_state() {
    let now = calibration::stamp("2026-10-12T08:00:00Z").unwrap();
    let rows: Vec<Value> = fixture("calibration.json")
        .iter()
        .map(|row| json!({"at": row[0], "kind": row[1], "cell": row[2], "n": row[3], "value": row[7], "source": row[8]}))
        .collect();
    let state = State {
        calibration: &calibration::read(&rows),
        denials: &jsonl(std::path::Path::new(&format!("{FIXTURES}/spawn-gate-denials.jsonl"))),
        scores: &jsonl(std::path::Path::new(&format!("{FIXTURES}/goal-scores.jsonl"))),
    };
    let expected = std::fs::read_to_string(format!("{FIXTURES}/expected.txt")).unwrap();
    assert_eq!(render(&state, now).join("\n") + "\n", expected);
}

#[test]
fn test_empty_state_says_so() {
    let now = calibration::stamp("2026-10-12T08:00:00Z").unwrap();
    let state = State { calibration: &[], denials: &[], scores: &[] };
    assert_eq!(
        render(&state, now),
        [
            "calibration: no calibration yet",
            "spawn-gate denials 7d: 0 (capacity 0, flow 0, pace 0)",
            "goal scores: none scored yet",
            "goal ETA: none predicted yet",
        ]
    );
}

#[test]
fn a_partial_goal_score_reports_its_landed_boxes_and_a_closed_one_its_finish() {
    let partial = json!({"predicted_at": "2026-11-05T11:00:00Z", "predicted_finish": "2026-11-05T13:00:00Z", "partial": true, "landed": ["north#5/1"], "boxes": 3, "actual_finish": "2026-11-05T12:00:00Z"});
    let closed = json!({"predicted_at": "2026-11-05T11:00:00Z", "predicted_finish": "2026-11-05T13:00:00Z", "partial": false, "landed": ["north#5/1", "wisp#2/1"], "boxes": 2, "actual_finish": "2026-11-05T13:30:00Z"});
    let rows = [
        json!({"at": "2026-11-05T12:00:00Z", "kind": "goal_actual", "cell": "north", "n": 1, "value": 2.6667, "source": partial.to_string()}),
        json!({"at": "2026-11-05T13:30:00Z", "kind": "goal_actual", "cell": "wisp", "n": 2, "value": 1.5, "source": closed.to_string()}),
    ];
    assert_eq!(
        calibration::goal_eta_lines(&calibration::read(&rows)),
        [
            "goal ETA:",
            "  north landed 1 of 3 boxes by 2026-11-05T12:00:00Z (actual/predicted 2.67, predicted at 2026-11-05T11:00:00Z)",
            "  wisp finished 2026-11-05T13:30:00Z vs predicted 2026-11-05T13:00:00Z (actual/predicted 1.50, predicted at 2026-11-05T11:00:00Z)",
        ]
    );
}
