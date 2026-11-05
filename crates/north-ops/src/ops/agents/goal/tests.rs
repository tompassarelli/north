use std::path::Path;

use rusqlite::Connection;
use serde_json::{Value, json};

use super::actual::actual_row;
use super::estimate::{Estimate, estimate};
use super::metrics::delivery_metrics;
use super::plan::record;
use super::schedule::{Task, schedule};
use super::stamp;
use crate::threads::calibration::box_durations;

const FIXTURE: &str = include_str!("../../../../../../harness/lib/fixtures/goal-eta-2026-10-12.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}

fn ledger(fixture: &Value) -> Vec<Value> {
    let fields: Vec<&str> = fixture["ledger_fields"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    fixture["ledger"].as_array().unwrap().iter().map(|row| Value::Object(fields.iter().zip(row.as_array().unwrap()).map(|(field, cell)| ((*field).to_owned(), cell.clone())).collect())).collect()
}

fn replay(goal: &Value, ledger: &[Value]) -> Estimate {
    let start = stamp(goal["start"].as_str().unwrap()).unwrap();
    let rows: Vec<Value> = ledger
        .iter()
        .filter(|row| stamp(row["ended"].as_str().unwrap()).unwrap() < start)
        .map(|row| {
            let mut row = row.clone();
            if row["landed_at"].as_str().and_then(stamp).is_some_and(|at| at >= start) {
                row["landed"] = json!(0);
                row["landed_at"] = Value::Null;
            }
            row
        })
        .collect();
    let measure = |facets: &[String]| box_durations(Path::new("/nonexistent"), start, facets, Some(&rows)).map_err(|_| "measure failed".to_owned());
    estimate("north", goal["issues"].as_array().unwrap(), &measure, goal["metrics"]["median_brief_to_landed_min"].as_f64(), goal["width"].as_u64().unwrap() as usize, start).unwrap()
}

fn off(finish: &str, actual: &str) -> f64 {
    (stamp(finish).unwrap().unix_nanos() - stamp(actual).unwrap().unix_nanos()).abs() as f64 / 60e9
}

fn paths(boxes: &[super::boxes::GoalBox]) -> Vec<f64> {
    fn walk(at: usize, boxes: &[super::boxes::GoalBox]) -> Vec<f64> {
        let own = boxes[at].eta.unwrap();
        let children: Vec<usize> = (0..boxes.len()).filter(|child| boxes[*child].deps.contains(&boxes[at].key)).collect();
        if children.is_empty() {
            return vec![own];
        }
        children.into_iter().flat_map(|child| walk(child, boxes)).map(|rest| own + rest).collect()
    }
    (0..boxes.len()).filter(|at| boxes[*at].deps.is_empty()).flat_map(|at| walk(at, boxes)).collect()
}

#[test]
fn goals_of_12_oct_replayed_from_their_start() {
    let fixture = fixture();
    let ledger = ledger(&fixture);
    let recorded = [("policy-lead harness goal", 450.0, "2026-10-12T12:54:00Z", 978.0, 26, 22), ("north#35", 62.0, "2026-10-12T08:07:00Z", 310.0, 5, 22)];
    for goal in fixture["goals"].as_array().unwrap() {
        let name = goal["goal"].as_str().unwrap();
        let result = replay(goal, &ledger);
        let (_, minutes, finish, serial, boxes, width) = *recorded.iter().find(|entry| entry.0 == name).unwrap();
        assert_eq!((result.minutes, result.finish.as_str(), result.serial_minutes, result.boxes, result.width), (Some(minutes), finish, serial, boxes, width), "{name}");
        assert!(stamp(&result.finish) <= stamp(goal["target"].as_str().unwrap()), "{name}");
        assert!(stamp(&result.finish) < stamp(goal["hand"].as_str().unwrap()), "{name}");
        if name == "north#35" {
            assert!(off(&result.finish, goal["actual"].as_str().unwrap()) < off(goal["m_predicted"].as_str().unwrap(), goal["actual"].as_str().unwrap()));
        } else {
            let longest = paths(&result.detail).into_iter().fold(0.0, f64::max);
            assert_eq!(result.minutes, Some(longest));
        }
    }
}

#[test]
fn goal_forecast_scales_boxes_by_the_median_not_the_coverage_quantile() {
    let scratch = tempfile::tempdir().unwrap();
    let path = scratch.path().join("ledger.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE runs (agent TEXT, role TEXT, brief_text TEXT, minutes REAL, item TEXT, category TEXT, outcome TEXT, staffed TEXT, started TEXT, ended TEXT, landed INT, landed_at TEXT, cost_usd REAL, tier TEXT, turns INT);
         CREATE TABLE calibration (at TEXT, kind TEXT, cell TEXT, n INT, p25 REAL, median REAL, p75 REAL, value REAL, source TEXT);",
    )
    .unwrap();
    let now = crate::foundation::clock::Timestamp::now();
    db.execute("INSERT INTO calibration VALUES (?1, 'box', 'all', 563, 0.31, 0.78, 1.99, 1.99, 'measured')", [now.iso()]).unwrap();
    drop(db);
    let measured = box_durations(&path, now, &["Category: feature".to_owned()], None).ok().unwrap();
    assert_eq!(measured["scale"].as_f64(), Some(0.78), "{}", measured["label"]);
}

fn run(agent: &str, item: &str, cost: f64) -> Value {
    json!({"agent": agent, "item": item, "role": "worker", "outcome": "done", "landed": 1, "follows": null,
           "staffed": "2026-10-11T00:00:00Z", "started": "2026-10-11T00:00:00Z", "ended": "2026-10-11T00:20:00Z",
           "landed_at": "2026-10-11T00:20:00Z", "cost_usd": cost})
}

#[test]
fn delivery_metrics_count_done_boxes_concurrency_landing_time_and_box_cost() {
    let rows = [run("a", "north#2", 1.46), run("b", "wisp#3", 3.54)];
    let alone = delivery_metrics(&rows, Some("north"));
    assert_eq!((alone.done_boxes, alone.concurrency), (1, 1));
    assert_eq!(alone.median_brief_to_landed_min, Some(20.0));
    assert_eq!(alone.median_cost_per_done_usd, Some(1.46));
    let mut retry = run("a0", "north#2", 0.54);
    retry.as_object_mut().unwrap().extend([
        ("outcome".to_owned(), json!("not done")),
        ("landed".to_owned(), json!(0)),
        ("landed_at".to_owned(), Value::Null),
        ("staffed".to_owned(), json!("2026-10-10T23:40:00Z")),
        ("started".to_owned(), json!("2026-10-10T23:40:00Z")),
        ("ended".to_owned(), json!("2026-10-10T23:55:00Z")),
    ]);
    let with_retry = delivery_metrics(&[rows[0].clone(), rows[1].clone(), retry], Some("north"));
    assert_eq!(with_retry.median_cost_per_done_usd, Some(2.0));
}

fn task(key: &str, eta: f64, deps: &[&str]) -> Task {
    Task { key: key.to_owned(), eta, deps: deps.iter().map(|dep| (*dep).to_owned()).collect() }
}

#[test]
fn a_schedule_runs_the_longest_path_first_at_the_width_and_refuses_a_cycle() {
    let tasks = [task("a", 10.0, &[]), task("b", 10.0, &["a"]), task("c", 5.0, &[]), task("d", 5.0, &[])];
    assert_eq!(schedule(&tasks, 1, 0.0), Ok(30.0));
    assert_eq!(schedule(&tasks, 2, 0.0), Ok(20.0));
    assert_eq!(schedule(&tasks, 2, 1.0), Ok(22.0));
    assert_eq!(schedule(&tasks, 0, 0.0), Err("width must be at least 1".to_owned()));
    let cycle = [task("a", 1.0, &["b"]), task("b", 1.0, &["a"])];
    assert!(schedule(&cycle, 2, 0.0).unwrap_err().starts_with("dependency cycle at"));
}

#[test]
fn a_finished_goal_scores_its_first_prediction_against_the_last_close() {
    let prediction = json!({"kind": "goal_eta", "cell": "north", "at": "2026-10-12T08:00:00Z", "n": 3, "value": 60.0, "source": json!({"predicted_finish": "2026-10-12T09:00:00Z"}).to_string()});
    let closed = json!({"state": "CLOSED", "closedAt": "2026-10-12T10:00:00Z"});
    let row = actual_row("north", std::slice::from_ref(&prediction), None, std::slice::from_ref(&closed)).unwrap();
    assert_eq!(row["value"].as_f64(), Some(2.0));
    assert_eq!(serde_json::from_str::<Value>(row["source"].as_str().unwrap()).unwrap()["actual_minutes"], 120.0);
    assert!(actual_row("north", std::slice::from_ref(&prediction), Some("2026-10-12T08:30:00Z"), &[closed]).is_none());
    assert!(actual_row("north", &[prediction], None, &[json!({"state": "OPEN", "closedAt": null})]).is_none());
}

#[test]
fn a_calibration_that_outlives_its_timeout_warns_once_and_records_nothing() {
    let scratch = tempfile::tempdir().unwrap();
    let north = scratch.path().join("north");
    crate::foundation::shim::write_executable(&north, "#!/usr/bin/env bash\n[[ \"$1 $2 $3\" == \"threads calibrate --write\" ]] || exit 97\nexec sleep 6\n");
    let mut warnings = Vec::new();
    assert_eq!(record(&json!({}), &north, &mut warnings), None);
    assert_eq!(String::from_utf8(warnings).unwrap(), "warning: threads calibrate --write timed out after 5s; goal ETA row not recorded\n");
}
