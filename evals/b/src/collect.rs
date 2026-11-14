use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use crate::proc::{TOOLPATH, rfc3339_epoch, run, ssh};

const OPENCODE: &str = "opencode";
const STORE: &str = "opencode-steps.jsonl";
const PRICES: &str = include_str!("../../../crates/north-pace/src/router/prices.json");

/// The assistant messages of `conversation` in the store of the opencode host that served `name`, one JSON object a line: the host keeps opencode's data under `north-opencode-NAME` in the temp directory of the launch that `--scratch` named.
fn opencode_steps(name: &str, conversation: &str) -> String {
    let store = std::env::temp_dir().join(format!("north-opencode-{name}")).join("data/opencode/opencode.db");
    format!("PATH={TOOLPATH}:$PATH sqlite3 -readonly '{}' \"SELECT data FROM session_message WHERE session_id = '{conversation}' AND type = 'assistant' ORDER BY seq\"", store.display())
}

pub fn session(work: &Path, name: &str) {
    let dir = work.join(name);
    let _ = fs::create_dir_all(&dir);
    let shown = run("north", &["fleet", "show", name, "--json"], None).out;
    let _ = fs::write(dir.join("session.json"), &shown);
    let _ = fs::write(dir.join("spend.txt"), run("north", &["fleet", "spend", name], None).out);
    let session: Value = serde_json::from_str(&shown).unwrap_or(Value::Null);
    if let Some(conversation) = session["conversation"].as_str() {
        let (file, read) = match session["provider"].as_str() {
            Some(OPENCODE) => (STORE, opencode_steps(name, conversation)),
            _ => ("transcript.jsonl", format!("fd -t f -H -I -g '{conversation}.jsonl' ~/.local/state/north/profiles -x cat")),
        };
        let machine = machine(&session);
        let text = if machine == here() { crate::proc::sh(&read).out } else { ssh(&machine, &read).out };
        let _ = fs::write(dir.join(file), text);
    }
    let _ = fs::write(dir.join("gates.json"), aside_gates(name));
    let stopped = run("north", &["fleet", "stop", name], None);
    let forgot = run("north", &["fleet", "forget", name], None);
    let _ = fs::write(dir.join("teardown.log"), format!("{}{}{}{}", stopped.out, stopped.err, forgot.out, forgot.err));
}

fn aside_gates(name: &str) -> String {
    let listed: Value = serde_json::from_str(&run("north", &["gate", "list", "--aside", "--json"], None).out).unwrap_or(Value::Null);
    let own: Vec<&Value> = listed.as_array().into_iter().flatten().filter(|gate| gate["session"] == name).collect();
    json!(own).to_string()
}

pub fn machine(session: &Value) -> String {
    session["machine"].as_str().map_or_else(here, str::to_owned)
}

fn here() -> String {
    crate::proc::sh("hostname -s").out.trim().to_owned()
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

/// The cost and total tokens of a session whose usage its own store priced, else what `north fleet spend` printed for it.
pub fn cost(dir: &Path, usage: &Value) -> (Option<f64>, Option<u64>) {
    let total = usage["tokens"].as_object().map(|tokens| tokens.values().filter_map(Value::as_u64).sum::<u64>());
    usage["cost_usd"].as_f64().map_or_else(|| spend(dir), |dollars| (Some(dollars), total))
}

/// The tokens, models and first request of an opencode session's assistant messages, reasoning counted as output as north's opencode host counts it, priced at the list prices in `crates/north-pace/src/router/prices.json` because the Go plan records every request at zero.
fn opencode(text: &str) -> Value {
    let steps: Vec<Value> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    let prices: Value = serde_json::from_str(PRICES).unwrap_or(Value::Null);
    let model = |step: &Value| format!("{}/{}", step["model"]["providerID"].as_str().unwrap_or_default(), step["model"]["id"].as_str().unwrap_or_default());
    let count = |step: &Value, key: &str| step.pointer(key).and_then(Value::as_u64).unwrap_or(0);
    let mut tokens = [0u64; 4];
    let mut dollars = Some(0.0);
    for step in &steps {
        let used = [count(step, "/tokens/input"), count(step, "/tokens/output") + count(step, "/tokens/reasoning"), count(step, "/tokens/cache/read"), count(step, "/tokens/cache/write")];
        let price = &prices["models"][model(step)];
        let priced: Option<f64> = used.iter().zip(["input", "output", "cache_read", "cache_write"]).map(|(n, key)| Some(*n as f64 * price[key].as_f64()? / 1e6)).sum();
        dollars = dollars.zip(priced).map(|(a, b)| a + b);
        tokens.iter_mut().zip(used).for_each(|(total, n)| *total += n);
    }
    let unique = |pick: &dyn Fn(&Value) -> Option<String>| steps.iter().filter_map(pick).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    json!({
        "first_request_at": steps.first().and_then(|s| s.pointer("/time/created")?.as_f64()).map(|ms| ms / 1000.0),
        "tokens": {"input": tokens[0], "output": tokens[1], "cache_read": tokens[2], "cache_write": tokens[3]},
        "cost_usd": dollars.map(|d| (d * 1e6).round() / 1e6), "price_source": "crates/north-pace/src/router/prices.json",
        "model_requests": steps.len(), "handins": null, "refusal_mentions": null,
        "models": unique(&|s| Some(model(s))), "efforts": unique(&|s| s["model"]["variant"].as_str().map(str::to_owned)), "claude_code": [],
        "records": steps.len(),
    })
}

pub fn transcript(dir: &Path) -> Value {
    if let Ok(text) = fs::read_to_string(dir.join(STORE)) {
        return opencode(&text);
    }
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
    let refusals = tools.iter().filter(|(input, t)| !*input && (t.contains("NOT queued") || t.contains("did not land") || t.contains("refused"))).count();
    let unique = |key: &str| assistant.iter().filter_map(|r| r.pointer(key).and_then(Value::as_str)).collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>();
    json!({
        "first_request_at": assistant.first().and_then(|r| r["timestamp"].as_str()).and_then(rfc3339_epoch),
        "tokens": {"input": tokens[0], "output": tokens[1], "cache_read": tokens[2], "cache_write": tokens[3]},
        "model_requests": seen.len(), "handins": crate::handins::count(&records), "refusal_mentions": refusals,
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
            let verdict = all.iter().find(|s| s["name"] == "private-kind verdict" && s["parent"].as_str() == Some(id));
            json!({"queued_at": landing["start"], "decided_at": landing["end"], "attributes": landing["attributes"],
                   "queue_wait_s": queued.and_then(|q| Some(q["end"].as_f64()? - q["start"].as_f64()?)),
                   "verdict_s": verdict.and_then(|v| Some(v["end"].as_f64()? - v["start"].as_f64()?))})
        })
        .collect();
    Value::Array(attempts)
}

pub fn last_words(dir: &Path) -> String {
    let session: Value = serde_json::from_str(&fs::read_to_string(dir.join("session.json")).unwrap_or_default()).unwrap_or(Value::Null);
    let text = session["entries"].as_array().into_iter().flatten().rev().find(|e| e["kind"] == "assistant").and_then(|e| e["text"].as_str()).unwrap_or_default();
    text.chars().take(600).collect()
}
