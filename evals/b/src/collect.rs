use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::proc::{rfc3339_epoch, run, ssh};

pub fn session(work: &Path, name: &str) {
    let dir = work.join(name);
    let _ = fs::create_dir_all(&dir);
    let shown = run("north", &["fleet", "show", name, "--json"], None).out;
    let _ = fs::write(dir.join("session.json"), &shown);
    let _ = fs::write(dir.join("spend.txt"), run("north", &["fleet", "spend", name], None).out);
    let session: Value = serde_json::from_str(&shown).unwrap_or(Value::Null);
    if let (Some(machine), Some(conversation)) = (session["machine"].as_str(), session["conversation"].as_str()) {
        let find = format!("fd -t f -H -I -g '{conversation}.jsonl' ~/.local/state/north/profiles -x cat");
        let text = if machine == crate::proc::sh("hostname -s").out.trim() { crate::proc::sh(&find).out } else { ssh(machine, &find).out };
        let _ = fs::write(dir.join("transcript.jsonl"), text);
    }
    let stopped = run("north", &["fleet", "stop", name], None);
    let forgot = run("north", &["fleet", "forget", name], None);
    let _ = fs::write(dir.join("teardown.log"), format!("{}{}{}{}", stopped.out, stopped.err, forgot.out, forgot.err));
}

pub fn spend(dir: &Path) -> (Option<f64>, Option<u64>) {
    let text = fs::read_to_string(dir.join("spend.txt")).unwrap_or_default();
    let dollars = text.split('$').nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|v| v.parse().ok());
    let tokens = text.split('(').nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|v| v.parse().ok());
    (dollars, tokens)
}

fn tool_texts(record: &Value) -> Vec<(bool, String)> {
    record["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| match part["type"].as_str() {
            Some("tool_use") => Some((true, part["input"].to_string())),
            Some("tool_result") => Some((false, part["content"].to_string())),
            _ => None,
        })
        .collect()
}

pub fn transcript(dir: &Path) -> Value {
    let text = fs::read_to_string(dir.join("transcript.jsonl")).unwrap_or_default();
    let records: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let assistant: Vec<&Value> = records.iter().filter(|r| r["type"] == "assistant" && r["message"]["usage"].is_object()).collect();
    let mut seen = BTreeSet::new();
    let mut tokens = [0u64; 4];
    for record in assistant.iter().rev() {
        if !seen.insert(record["message"]["id"].as_str().unwrap_or_default().to_owned()) {
            continue;
        }
        let usage = &record["message"]["usage"];
        for (slot, key) in tokens.iter_mut().zip(["input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"]) {
            *slot += usage[key].as_u64().unwrap_or(0);
        }
    }
    let tools: Vec<(bool, String)> = records.iter().filter(|r| r["type"] == "assistant" || r["type"] == "user").flat_map(tool_texts).collect();
    let handins = tools.iter().filter(|(input, t)| *input && t.contains("safe-push --to main")).count();
    let conflicts = tools.iter().filter(|(input, t)| !*input && t.to_lowercase().contains("conflict") && (t.contains("safe-push") || t.contains("NOT queued") || t.contains("north land"))).count();
    let refusals = tools.iter().filter(|(input, t)| !*input && (t.contains("NOT queued") || t.contains("did not land") || t.contains("refused"))).count();
    let unique = |key: &str| assistant.iter().filter_map(|r| r.pointer(key).and_then(Value::as_str)).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    json!({
        "first_request_at": assistant.first().and_then(|r| r["timestamp"].as_str()).and_then(rfc3339_epoch),
        "tokens": {"input": tokens[0], "output": tokens[1], "cache_read": tokens[2], "cache_write": tokens[3]},
        "handins": handins, "conflict_mentions": conflicts, "refusal_mentions": refusals,
        "models": unique("/message/model"), "efforts": unique("/effort"), "claude_code": unique("/version"),
        "records": records.len(),
    })
}

pub fn spans(queue: Option<&Path>, branch: &str, since: f64) -> Value {
    let Some(text) = queue.and_then(|q| fs::read_to_string(q.join("logs/spans.jsonl")).ok()) else { return json!([]) };
    let all: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let landings: Vec<&Value> = all.iter().filter(|s| s["category"] == "landing" && s["name"] == branch && s["start"].as_f64().is_some_and(|start| start >= since)).collect();
    let attempts: Vec<Value> = landings
        .iter()
        .map(|landing| {
            let id = landing["id"].as_str().unwrap_or_default();
            let queued = all.iter().find(|s| s["name"] == "queued" && s["parent"].as_str() == Some(id));
            json!({"queued_at": landing["start"], "decided_at": landing["end"], "attributes": landing["attributes"],
                   "queue_wait_s": queued.and_then(|q| Some(q["end"].as_f64()? - q["start"].as_f64()?))})
        })
        .collect();
    Value::Array(attempts)
}

pub fn last_words(dir: &Path) -> String {
    let session: Value = serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap_or_default()).unwrap_or(Value::Null);
    let text = session["entries"].as_array().into_iter().flatten().rev().find(|e| e["kind"] == "assistant").and_then(|e| e["text"].as_str()).unwrap_or_default();
    text.chars().take(600).collect()
}
