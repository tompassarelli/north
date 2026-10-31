use serde_json::{Value, json};

use super::{State, arms, calibration, jsonl, render};

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
    let runs: Vec<Value> = fixture("runs.json")
        .iter()
        .map(|row| json!({"agent": row[0], "arm": row[1], "follows": row[2], "outcome": row[3], "expects_landing": row[4], "landed": row[5], "item": null}))
        .collect();
    let state = State {
        calibration: &calibration::read(&rows),
        denials: &jsonl(std::path::Path::new(&format!("{FIXTURES}/spawn-gate-denials.jsonl"))),
        scores: &jsonl(std::path::Path::new(&format!("{FIXTURES}/goal-scores.jsonl"))),
        runs: &arms::read(&runs),
    };
    let expected = std::fs::read_to_string(format!("{FIXTURES}/expected.txt")).unwrap();
    assert_eq!(render(&state, now).join("\n") + "\n", expected);
}

#[test]
fn test_empty_state_says_so() {
    let now = calibration::stamp("2026-10-12T08:00:00Z").unwrap();
    let state = State { calibration: &[], denials: &[], scores: &[], runs: &[] };
    assert_eq!(
        render(&state, now),
        [
            "calibration: no calibration yet",
            "escalation arms: escalate 0/0 [0%-100%], cheap-fix 0/0 [0%-100%], split 0/0 [0%-100%] (needs 15 runs per arm)",
            "spawn-gate denials 7d: 0 (capacity 0, flow 0, pace 0)",
            "goal scores: none scored yet",
            "goal ETA: none predicted yet",
        ]
    );
}
