use serde_json::{Value, json};

use super::IngestUsage;

fn event(used: f64) -> Value {
    json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "rateLimitType": "five_hour", "unifiedWindows": {"five_hour": {"utilization": used, "resetsAt": 1792049400}}}})
}

fn rows(batch: &str) -> Vec<Value> {
    batch.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn a_claude_rate_limit_event_is_a_usage_batch_under_the_sessions_account() {
    let sink = IngestUsage::default();
    let written = rows(&sink.batch("claude", "c", &event(0.5), 1_792_000_000.0).unwrap());
    assert_eq!(written.iter().map(|row| (row["kind"].as_str().unwrap(), row["name"].as_str().unwrap())).collect::<Vec<_>>(), [("window", "session"), ("status", "session")]);
    assert!(written.iter().all(|row| row["type"] == "usage" && row["account"] == "c" && row["provider"] == "claude"));
}

#[test]
fn a_repeat_of_the_last_readings_is_not_written_again_and_a_changed_one_is() {
    let sink = IngestUsage::default();
    assert!(sink.batch("claude", "b", &event(0.1), 1.0).is_some());
    assert!(sink.batch("claude", "b", &event(0.1), 6.0).is_none());
    assert!(sink.batch("claude", "b", &event(0.2), 11.0).is_some());
    assert!(sink.batch("claude", "c", &event(0.2), 11.0).is_some());
}

#[test]
fn frames_that_are_not_a_claude_rate_limit_event_write_nothing() {
    let sink = IngestUsage::default();
    assert!(sink.batch("codex", "c", &event(0.5), 1.0).is_none());
    assert!(sink.batch("claude", "c", &json!({"type": "assistant"}), 1.0).is_none());
}
