use std::fs::{self, OpenOptions};
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use serde_json::json;

use super::{line, read, record_arguments};
use crate::foundation::clock::Timestamp;

const NOW: f64 = 1_793_847_600.0;
const MINUTE: f64 = 60.0;

fn ticket(directory: &Path, name: &str, queued_ago: f64, first_commit_ago: Option<f64>, verdict: &str, decided_ago: f64) {
    let path = directory.join(format!("{name}.json"));
    let mut body = json!({"branch": name, "queued_at": Timestamp::from_unix_seconds_f64(NOW - queued_ago * MINUTE).iso(), "verdict": verdict});
    if let Some(ago) = first_commit_ago {
        body["first_commit_secs"] = json!((NOW - ago * MINUTE) as i64);
    }
    fs::create_dir_all(directory).unwrap();
    fs::write(&path, body.to_string()).unwrap();
    OpenOptions::new().write(true).open(&path).unwrap().set_modified(UNIX_EPOCH + Duration::from_secs_f64(NOW - decided_ago * MINUTE)).unwrap();
}

#[test]
fn medians_and_counts_come_from_the_tickets_decided_in_the_window() {
    let queue = tempfile::tempdir().unwrap();
    let done = queue.path().join("done");
    ticket(&done, "a", 12.0, Some(40.0), "landed aaa", 10.0);
    ticket(&done, "b", 25.0, Some(65.0), "landed bbb", 20.0);
    ticket(&done, "c", 31.0, None, "landed ccc", 30.0);
    ticket(&done, "d", 48.0, Some(130.0), "landed ddd", 40.0);
    ticket(&done, "e", 50.0, None, "failed /logs/e.log", 45.0);
    ticket(&done, "f", 50.0, None, "failed interaction with a: /logs/f.log", 45.0);
    ticket(&done, "g", 50.0, None, "conflict docs/features.md", 45.0);
    ticket(&done, "h", 50.0, None, "replaced by a newer ticket for this lane", 45.0);
    ticket(&done, "old", 200.0, Some(300.0), "landed old", 90.0);
    ticket(queue.path(), "1793847500000000-waiting", 1.0, None, "", 0.0);
    fs::write(done.join("torn.json"), "{").unwrap();

    let reading = read("north", queue.path(), NOW, 60);

    assert_eq!(reading.landed, 4);
    assert_eq!(reading.queued_to_main_p50_minutes, Some(3.5));
    assert_eq!(reading.queued_to_main_p90_minutes, Some(8.0));
    assert_eq!(reading.first_commit_to_main_p50_minutes, Some(45.0));
    assert_eq!(reading.first_commit_to_main_p90_minutes, Some(90.0));
    assert_eq!(reading.first_commit_samples, 3);
    assert_eq!((reading.refused_alone, reading.conflicts, reading.queued_now), (1, 1, 1));
    assert_eq!(read("north", queue.path(), NOW, 120).landed, 5);
    assert!(line(&reading).contains("4 lanes landed, queued->main p50 3.5 p90 8.0 min, first commit->main p50 45.0 p90 90.0 min over 3 lanes, 1 refused alone, 1 conflicts, 1 queued now"));
}

#[test]
fn an_empty_window_reads_zero_lanes_and_records_no_medians() {
    let queue = tempfile::tempdir().unwrap();
    let reading = read("north", queue.path(), NOW, 60);
    assert_eq!((reading.landed, reading.queued_to_main_p50_minutes, reading.queued_now), (0, None, 0));
    let words = record_arguments(&reading, NOW).join(" ");
    assert!(words.starts_with("land-train tempo --repo north --at 1793847600 --window-minutes 60 --landed 0"), "{words}");
    assert!(!words.contains("--queued-p50"), "{words}");
}
