use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use rusqlite::Connection;
use serde_json::json;

use super::*;

fn context(model: &str) -> String {
    json!({"type": "turn_context", "payload": {"model": model, "effort": "high"}}).to_string()
}

fn complete(at: &str, error: serde_json::Value) -> String {
    json!({"type": "event_msg", "timestamp": at, "payload": {"type": "task_complete", "error": error}}).to_string()
}

fn capacity() -> serde_json::Value {
    json!({"codex_error_info": "server_overloaded", "message": "x"})
}

fn rollout(directory: &Path, name: &str, lines: &[String]) -> String {
    fs::create_dir_all(directory).unwrap();
    let path = directory.join(name);
    fs::write(&path, lines.join("\nnot json\n")).unwrap();
    path.to_string_lossy().into_owned()
}

fn read(paths: &[String]) -> Vec<Outcome> {
    outcomes(paths, 0.0)
}

#[test]
fn a_turn_is_capacity_by_error_info_or_message_error_when_it_failed_otherwise_and_ok_when_it_did_not() {
    let directory = tempfile::tempdir().unwrap();
    let path = rollout(
        directory.path(),
        "rollout-a.jsonl",
        &[
            context("sol"),
            complete("2026-11-04T10:00:00Z", capacity()),
            complete("2026-11-04T10:01:00Z", json!({"message": "Selected model is at capacity, retry"})),
            complete("2026-11-04T10:02:00Z", json!({"message": "boom"})),
            complete("2026-11-04T10:03:00Z", json!(null)),
        ],
    );
    let kinds: Vec<&str> = read(&[path]).iter().map(|row| row.kind).collect();
    assert_eq!(kinds, ["capacity", "capacity", "error", "ok"]);
}

#[test]
fn events_print_capacity_turns_in_time_order_across_rollouts_with_the_model_and_effort_in_force() {
    let directory = tempfile::tempdir().unwrap();
    let late = rollout(directory.path(), "rollout-late.jsonl", &[context("luna"), complete("2026-11-04T10:09:00Z", capacity())]);
    let early = rollout(directory.path(), "rollout-early.jsonl", &[context("sol"), complete("2026-11-04T10:01:00Z", capacity()), complete("2026-11-04T10:02:00Z", json!(null))]);
    let printed = events_json(&read(&[late.clone(), early.clone()]));
    assert_eq!(
        printed,
        format!("[{{\"at\": \"2026-11-04T10:01:00Z\", \"model\": \"sol\", \"effort\": \"high\", \"outcome\": \"capacity\", \"rollout\": \"{early}\"}}, {{\"at\": \"2026-11-04T10:09:00Z\", \"model\": \"luna\", \"effort\": \"high\", \"outcome\": \"capacity\", \"rollout\": \"{late}\"}}]")
    );
    assert_eq!(events_json(&[]), "[]");
}

#[test]
fn a_later_answer_clears_the_model_and_the_status_names_the_earliest_failure_still_open() {
    let directory = tempfile::tempdir().unwrap();
    let path = rollout(
        directory.path(),
        "rollout-a.jsonl",
        &[
            context("sol"),
            complete("2026-11-04T10:00:00Z", capacity()),
            complete("2026-11-04T10:01:00Z", capacity()),
            context("luna"),
            complete("2026-11-04T10:02:00Z", capacity()),
        ],
    );
    assert_eq!(status_json(&read(std::slice::from_ref(&path))),"{\"codex\": {\"since\": \"2026-11-04T10:00:00Z\", \"model\": \"sol\"}}");
    let recovered = rollout(directory.path(), "rollout-b.jsonl", &[context("sol"), complete("2026-11-04T10:05:00Z", json!(null))]);
    assert_eq!(status_json(&read(&[path, recovered])), "{\"codex\": {\"since\": \"2026-11-04T10:02:00Z\", \"model\": \"luna\"}}");
    assert_eq!(status_json(&[]), "{}");
}

#[test]
fn turns_before_the_window_are_ignored() {
    let directory = tempfile::tempdir().unwrap();
    let path = rollout(directory.path(), "rollout-a.jsonl", &[context("sol"), complete("2026-11-04T10:00:00Z", capacity())]);
    let after = Timestamp::parse_rfc3339("2026-11-04T10:00:01Z").unwrap().unix_seconds_f64();
    assert!(outcomes(&[path], after).is_empty());
}

#[test]
fn without_a_state_database_rollouts_are_found_under_sessions_by_modification_time() {
    let directory = tempfile::tempdir().unwrap();
    let nested = directory.path().join("sessions/2026/11/04");
    let fresh = rollout(&nested, "rollout-fresh.jsonl", &[]);
    let stale = rollout(&nested, "rollout-stale.jsonl", &[]);
    rollout(&nested, "notes.jsonl", &[]);
    fs::File::options().write(true).open(&stale).unwrap().set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() - 600.0;
    assert_eq!(rollouts(directory.path(), since).unwrap(), [fresh]);
}

#[test]
fn with_a_state_database_the_threads_updated_since_the_window_name_the_rollouts() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("sqlite")).unwrap();
    let db = Connection::open(directory.path().join("sqlite/state_5.sqlite")).unwrap();
    db.execute("CREATE TABLE threads(rollout_path TEXT, updated_at INTEGER)", []).unwrap();
    db.execute("INSERT INTO threads VALUES ('/r/new.jsonl', 2000), ('/r/old.jsonl', 100)", []).unwrap();
    drop(db);
    fs::create_dir_all(directory.path().join("sessions")).unwrap();
    rollout(&directory.path().join("sessions"), "rollout-ignored.jsonl", &[]);
    assert_eq!(rollouts(directory.path(), 1000.5).unwrap(), ["/r/new.jsonl"]);
}
