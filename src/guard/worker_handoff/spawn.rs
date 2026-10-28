use super::{Env, valid_id};
use crate::foundation::clock::Timestamp;
use crate::hook::HookInput;
use crate::platform::pyjson::{self, Value};
use serde_json::json;
use std::io::{ErrorKind, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INGEST_WAIT: Duration = Duration::from_secs(8);
const ORIGIN: &str = "claude.PostToolUse.Agent";

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        Value::Number { raw, .. } => raw.parse::<f64>() != Ok(0.0),
        Value::String(text) => !text.codepoints.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
    }
}

fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(flag) => if *flag { "True" } else { "False" }.into(),
        Value::Number { raw, .. } => raw.clone(),
        Value::String(text) => text.lossy().into_owned(),
        other => pyjson::dumps(other),
    }
}

fn plain(value: &Value) -> serde_json::Value {
    serde_json::from_str(&pyjson::dumps(value)).unwrap_or(serde_json::Value::Null)
}

fn row(input: &HookInput, now: f64) -> Option<serde_json::Value> {
    let data = &input.data;
    let (args, result) = (&data["tool_input"], &data["tool_response"]);
    if !args.is_object() || !result.is_object() || result["status"] != "async_launched" {
        return None;
    }
    let kind = args["subagent_type"].as_str().filter(|kind| kind.starts_with("worker"))?;
    let brief = args["prompt"].as_str()?;
    let child = result["agentId"].as_str().filter(|id| valid_id(id))?;
    let tier = kind.strip_prefix("worker").unwrap_or(kind);
    let tier = tier.strip_prefix('-').unwrap_or(tier);
    let staffed = Timestamp::from_unix_seconds_f64(now).iso_micros_utc_offset();
    Some(json!({
        "id": child, "session": plain(&data["session_id"]), "status": "running", "role": "worker",
        "tier": if tier.is_empty() { "medium" } else { tier },
        "model": plain(&result["resolvedModel"]), "started": staffed, "staffed": staffed,
        "brief": brief, "brief_text": brief, "brief_origin": ORIGIN,
    }))
}

fn confirm(input: &HookInput, env: &Env) -> Result<(), String> {
    let Some(confirm) = env.confirm else {
        return Ok(());
    };
    let data = &input.data;
    let state = std::env::var_os("SPAWN_CAPACITY_HOME")
        .filter(|home| !home.is_empty())
        .map(|home| std::path::PathBuf::from(home).join(".local/state"))
        .or_else(|| std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map(Into::into))
        .unwrap_or_else(|| env.home_dir().join(".local/state"));
    confirm(&json!({
        "op": "confirm", "directory": state.join("agents"), "session": plain(&data["session_id"]),
        "tool_use_id": plain(&data["tool_use_id"]), "launched": true,
    }))
}

pub(super) fn record(input: &HookInput, env: &Env) -> Option<String> {
    let confirming = input.data["tool_response"].is_object() && truthy(&input.data["tool_response"]["agentId"]);
    let row = row(input, env.now);
    if !confirming && row.is_none() {
        return None;
    }
    if (env.off)() {
        return None;
    }
    let data = &input.data;
    let lead = py_str(if truthy(&data["agent_id"]) { &data["agent_id"] } else { &data["session_id"] });
    if confirming && let Err(error) = confirm(input, env) {
        return Some(format!("spawn reservation confirmation failed for lead {lead}: {error}"));
    }
    let miss = ingest(&format!("{}\n", row?)).err()?;
    Some(format!("spawn recording boundary miss for lead {lead}: {miss}"))
}

fn failure(error: std::io::Error) -> &'static str {
    match error.kind() {
        ErrorKind::NotFound => "FileNotFoundError",
        ErrorKind::PermissionDenied => "PermissionError",
        _ => "OSError",
    }
}

fn ingest(line: &str) -> Result<(), &'static str> {
    let mut child = Command::new("threads")
        .arg("ingest")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(failure)?;
    let mut stdin = child.stdin.take().ok_or("OSError")?;
    let payload = line.to_owned();
    std::thread::spawn(move || stdin.write_all(payload.as_bytes()));
    let deadline = Instant::now() + INGEST_WAIT;
    loop {
        match child.try_wait().map_err(failure)? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => return Err("CalledProcessError"),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("TimeoutExpired");
            }
        }
    }
}
