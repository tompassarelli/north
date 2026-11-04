use serde_json::json;

use super::super::database::insert_capacity_run;
use super::super::estimate::task_signature;
use super::super::model::tests::database;
use super::{CalibrationRun, calibrate, calibration_runs, calibration_table, parse_window_seconds};

fn run(key: &str, predicted_wall: f64, wall_p90: f64, actual_wall: f64, predicted_memory: f64, actual_memory: f64) -> CalibrationRun {
    CalibrationRun {
        key: key.to_owned(),
        host: "laptop".to_owned(),
        basis: Some("owner-shape".to_owned()),
        predicted_wall: Some(predicted_wall),
        predicted_wall_p90: Some(wall_p90),
        actual_wall: Some(actual_wall),
        predicted_memory: Some(predicted_memory),
        actual_memory: Some(actual_memory),
        censored: false,
    }
}

#[test]
fn calibration_reports_error_quantiles_and_p90_coverage_per_key_and_host_and_flags_drift() {
    let mut runs = vec![run("steady", 100.0, 120.0, 100.0, 1000.0, 900.0), run("steady", 100.0, 120.0, 110.0, 1000.0, 1000.0), run("steady", 100.0, 120.0, 90.0, 1000.0, 950.0)];
    runs.extend([run("drifting", 100.0, 150.0, 100.0, 1000.0, 1000.0), run("drifting", 100.0, 150.0, 300.0, 1000.0, 1000.0)]);
    runs.push(run("uncovered", 100.0, 105.0, 104.0, 1000.0, 1200.0));
    runs.push(CalibrationRun { key: "unpredicted".to_owned(), host: "gex45".to_owned(), actual_wall: Some(5.0), actual_memory: Some(10.0), censored: true, ..CalibrationRun::default() });
    let report = calibrate(&runs);
    let keys: Vec<(&str, &str)> = report.iter().map(|row| (row.key.as_str(), row.host.as_str())).collect();
    assert_eq!(keys, [("drifting", "laptop"), ("steady", "laptop"), ("uncovered", "laptop"), ("unpredicted", "gex45")]);
    let steady = &report[1];
    assert_eq!(steady.runs, 3);
    assert!((steady.wall_error.p90 - 10.0 / 90.0).abs() < 1e-9);
    assert_eq!((steady.wall_coverage, steady.memory_coverage), (Some(1.0), Some(1.0)));
    assert!(!steady.drift);
    assert!(report[0].drift);
    assert!((report[0].wall_error.p90 - 2.0 / 3.0).abs() < 1e-9);
    assert_eq!(report[0].wall_coverage, Some(0.5));
    let uncovered = &report[2];
    assert!(uncovered.wall_error.p90 < 0.5 && uncovered.memory_error.p90 < 0.5);
    assert_eq!(uncovered.memory_coverage, Some(0.0));
    assert!(uncovered.drift);
    assert_eq!((report[3].wall_coverage, report[3].censored, report[3].drift), (None, 1, false));
    let table = calibration_table(&report);
    assert!(table.contains("WALL COV") && table.contains("MEM COV") && table.contains("DRIFT"));
}

#[test]
fn calibration_reads_only_keyed_runs_inside_the_window() {
    let db = database();
    let signature = task_signature(&["cargo".to_owned(), "test".to_owned()], None, "heavy", true);
    let entries = [("old", "2026-11-01T00:00:00.000Z", Some("north cargo test")), ("new", "2026-11-04T00:00:00.000Z", Some("north cargo test")), ("unkeyed", "2026-11-04T00:00:00.000Z", None)];
    for (source, at, key) in entries {
        let entry = json!({
            "at": at, "key": key, "host": "gex45", "wallSeconds": 10.0, "predictedWallSeconds": 12.0, "predictedWallP90Seconds": 15.0,
            "anonPeakMiB": 100.0, "predictedMemoryMiB": 90.0, "censored": true, "predictionBasis": "command",
        });
        insert_capacity_run(&db, source, &entry, &signature, None).expect("insert");
    }
    let found = calibration_runs(&db, "2026-11-03T00:00:00.000Z").expect("runs");
    let expected = CalibrationRun {
        key: "north cargo test".to_owned(),
        host: "gex45".to_owned(),
        basis: Some("command".to_owned()),
        predicted_wall: Some(12.0),
        predicted_wall_p90: Some(15.0),
        actual_wall: Some(10.0),
        predicted_memory: Some(90.0),
        actual_memory: Some(100.0),
        censored: true,
    };
    assert_eq!(found, vec![expected]);
}

#[test]
fn the_since_window_reads_seconds_minutes_hours_and_days() {
    assert_eq!(parse_window_seconds("24h"), Some(86_400.0));
    assert_eq!(parse_window_seconds("30m"), Some(1800.0));
    assert_eq!(parse_window_seconds("7d"), Some(604_800.0));
    assert_eq!(parse_window_seconds("24"), None);
    assert_eq!(parse_window_seconds("1w"), None);
}

#[test]
fn a_censored_run_is_a_memory_lower_bound_counting_only_as_a_coverage_miss_once_past_the_prediction() {
    let pressed = |actual: f64| CalibrationRun { censored: true, ..run("pressed", 100.0, 120.0, 100.0, 1000.0, actual) };
    let report = calibrate(&[pressed(900.0), pressed(1300.0), run("pressed", 100.0, 120.0, 100.0, 1000.0, 1000.0)]);
    assert_eq!(report[0].censored, 2);
    assert_eq!(report[0].memory_error.p90, 0.0);
    assert_eq!(report[0].memory_coverage, Some(0.5));
}
