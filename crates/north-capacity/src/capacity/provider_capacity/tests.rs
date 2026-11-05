use std::fs;
use std::path::Path;

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

// KEEP(contract): reads the external codex rollout format, classifying a turn as capacity by error info or message, other error, or ok
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
