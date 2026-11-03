use std::collections::HashSet;

use serde_json::{Map, Value, json};

use crate::foundation::clock::Timestamp;

const GAP_CAP_MS: i64 = 600_000;

pub(super) struct Activity {
    pub turns: usize,
    pub tool_calls: usize,
    pub active_min: f64,
}

fn tool_uses(record: &Value) -> impl Iterator<Item = &Value> {
    record["message"]["content"].as_array().into_iter().flatten().filter(|part| part["type"] == "tool_use")
}

fn human(record: &Value) -> bool {
    record["type"] == "user"
        && match &record["message"]["content"] {
            Value::String(_) => true,
            Value::Array(parts) => !parts.iter().any(|part| part["type"] == "tool_result"),
            _ => false,
        }
}

fn message_key(record: &Value, index: usize) -> String {
    [&record["message"]["id"], &record["requestId"]].into_iter().find(|value| value.is_string()).map_or_else(|| format!("line-{index}"), Value::to_string)
}

fn millis(record: &Value) -> Option<i64> {
    Timestamp::parse(record["timestamp"].as_str()?).ok().map(Timestamp::unix_millis)
}

pub(super) fn measure(lines: &[Value]) -> Activity {
    let mut messages = HashSet::new();
    let mut calls = HashSet::new();
    let mut active_ms = 0;
    let mut previous: Option<(i64, bool)> = None;
    for (index, line) in lines.iter().enumerate() {
        let kind = line["type"].as_str().unwrap_or("");
        if kind != "assistant" && kind != "user" {
            continue;
        }
        if kind == "assistant" && !line["message"]["usage"].is_null() {
            messages.insert(message_key(line, index));
        }
        if kind == "assistant" {
            for (position, part) in tool_uses(line).enumerate() {
                calls.insert(part["id"].as_str().map_or_else(|| format!("line-{index}-{position}"), str::to_owned));
            }
        }
        let Some(at) = millis(line) else { continue };
        if let Some((before, ended_turn)) = previous
            && !(ended_turn && human(line))
        {
            active_ms += (at - before).clamp(0, GAP_CAP_MS);
        }
        previous = Some((at, kind == "assistant" && tool_uses(line).next().is_none()));
    }
    Activity { turns: messages.len(), tool_calls: calls.len(), active_min: active_ms as f64 / 60_000.0 }
}

pub(super) fn insert(row: &mut Map<String, Value>, activity: &Activity, wall_seconds: i64) {
    let tenths = |minutes: f64| (minutes * 10.0).round() / 10.0;
    let wall = wall_seconds as f64 / 60.0;
    let active = activity.active_min.min(wall);
    row.insert("turns".into(), json!(activity.turns));
    row.insert("tool_calls".into(), json!(activity.tool_calls));
    row.insert("active_min".into(), json!(tenths(active)));
    row.insert("wait_min".into(), json!(tenths(wall - active)));
}
