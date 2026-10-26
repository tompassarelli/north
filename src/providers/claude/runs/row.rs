use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::{int, number, present};
use crate::foundation::clock::Timestamp;

static WAITING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*(waiting|i.ll wait|i.ll pick|the test run)").unwrap());
static ETA: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bETA\b[^0-9\n]{0,12}([0-9]+(?:\.[0-9]+)?)\s*(hours?|hrs?|h|minutes?|mins?|m)\b").unwrap());
static FLAWS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\baccepted flaws?:\s*([0-9]+)").unwrap());
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
static OUTCOMES: LazyLock<[(Regex, &str); 5]> = LazyLock::new(|| {
    [
        (Regex::new("HANDOFF").unwrap(), "handoff"),
        (Regex::new(r"(?i)^\W*split:").unwrap(), "split"),
        (Regex::new(r"(?i)^\W*(not|partly|mostly|half) done").unwrap(), "not done"),
        (Regex::new(r"(?i)^\W*blocked").unwrap(), "escalated"),
        (Regex::new(r"(?i)^\W*done\b").unwrap(), "done"),
    ]
});
const RUNNING_SECONDS: i64 = 7200;

pub(super) struct Transcript<'a> {
    pub lines: &'a [Value],
    pub id: &'a str,
    pub role: &'a str,
    pub tier: &'a str,
    pub since: &'a str,
    pub backfill: bool,
    pub now: Timestamp,
}

fn texts(record: &Value) -> String {
    match &record["message"]["content"] {
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part["type"] == "text")
            .map(|part| match &part["text"] {
                Value::String(text) => text.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn prefix(text: &str, length: usize) -> String {
    text.chars().take(length).collect()
}

fn seconds(text: &str) -> Option<i64> {
    Timestamp::parse(text).ok().map(Timestamp::unix_seconds)
}

fn finished(last: &Value) -> bool {
    let Some(parts) = last["message"]["content"].as_array().filter(|_| last["type"] == "assistant") else { return false };
    let text = texts(last);
    let length = text.chars().count();
    !parts.iter().any(|part| part["type"] == "tool_use") && length > 0 && (length > 200 || !WAITING.is_match(&text))
}

fn eta(brief: &str) -> Value {
    ETA.captures(brief).map_or(Value::Null, |found| {
        let amount: f64 = found[1].parse().unwrap_or(0.0);
        number(amount * if found[2].starts_with('h') { 60.0 } else { 1.0 })
    })
}

fn tokens(assistant: &[&Value]) -> i64 {
    let mut last: HashMap<String, &Value> = HashMap::new();
    for record in assistant {
        let key = [&record["requestId"], &record["message"]["id"]].into_iter().find(|value| present(value)).unwrap_or(&Value::Null);
        last.insert(key.to_string(), record);
    }
    last.values().map(|record| int(&record["message"]["usage"]["output_tokens"])).sum()
}

fn report(assistant: &[&Value]) -> Option<String> {
    let reports: Vec<String> = assistant
        .iter()
        .filter(|record| record["message"]["content"].as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "text")))
        .map(|record| texts(record))
        .collect();
    reports[reports.len().saturating_sub(3)..].iter().max_by_key(|text| text.chars().count()).cloned()
}

pub(super) fn build(transcript: &Transcript) -> Option<Map<String, Value>> {
    let lines = transcript.lines;
    let assistant: Vec<&Value> = lines.iter().filter(|line| line["type"] == "assistant" && !line["message"]["usage"].is_null()).collect();
    let last_usage = assistant.last()?;
    let tier = match transcript.role {
        "worker" => transcript.tier.to_owned(),
        _ => last_usage["message"]["model"].as_str().unwrap_or("unknown").to_owned(),
    };
    let last = lines.iter().rev().find(|line| line["type"] == "user" || line["type"] == "assistant")?;
    let done = finished(last);
    let brief = lines.iter().find(|line| line["type"] == "user").map_or_else(|| "null".to_owned(), texts);
    let times: Vec<&Value> = lines.iter().map(|line| &line["timestamp"]).filter(|at| present(at)).collect();
    let ended = times.last()?.as_str()?;
    if ended < transcript.since {
        return None;
    }
    let started = times[0].clone();
    let mut row = Map::new();
    row.insert("id".into(), json!(transcript.id));
    row.insert("session".into(), lines[0]["sessionId"].clone());
    row.insert("brief".into(), json!(prefix(&brief, 4000)));
    row.insert("started".into(), started.clone());
    row.insert("tier".into(), json!(tier));
    row.insert("eta_min".into(), eta(&brief));
    if !done && transcript.role == "worker" {
        let running = transcript.now.unix_seconds() - seconds(ended)? < RUNNING_SECONDS;
        row.insert("status".into(), json!(if running { "running" } else { "stale" }));
        return Some(row);
    }
    let report = report(&assistant)?;
    let head = prefix(&report, 400);
    let usage = &last_usage["message"]["usage"];
    row.insert("status".into(), json!(if done { "finished" } else { "active" }));
    row.insert("ended".into(), json!(ended));
    row.insert("actual_min".into(), json!((seconds(ended)? - seconds(started.as_str()?)?).div_euclid(60)));
    row.insert("tokens".into(), json!(tokens(&assistant)));
    row.insert("peak_ctx".into(), json!(int(&usage["input_tokens"]) + int(&usage["cache_read_input_tokens"]) + int(&usage["cache_creation_input_tokens"])));
    row.insert("outcome".into(), json!(OUTCOMES.iter().find(|(pattern, _)| pattern.is_match(&head)).map_or("unclear", |(_, outcome)| outcome)));
    row.insert("flaws".into(), FLAWS.captures(&report).map_or(Value::Null, |found| number(found[1].parse().unwrap_or(0.0))));
    row.insert("backfill".into(), json!(transcript.backfill));
    row.insert("report".into(), json!(prefix(&SPACE.replace_all(&head, " "), 160)));
    Some(row)
}
