use super::{Env, valid_id};
use crate::foundation::clock::Timestamp;
use crate::hook_input::HookInput;
use serde_json::{Value, json};
use std::io::{ErrorKind, Write};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

const ORIGIN: &str = "claude.PostToolUse.Agent";

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
    }
}

fn py_str(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(flag) => if *flag { "True" } else { "False" }.into(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn row(input: &HookInput, now: f64) -> Option<Value> {
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
        "id": child, "session": data["session_id"], "status": "running", "role": "worker",
        "tier": if tier.is_empty() { "medium" } else { tier },
        "model": result["resolvedModel"], "started": staffed, "staffed": staffed,
        "brief": brief, "brief_text": brief, "brief_origin": ORIGIN,
    }))
}

pub(super) fn record(input: &HookInput, env: &Env) -> Option<String> {
    let row = row(input, env.now)?;
    let data = &input.data;
    let lead = py_str(if truthy(&data["agent_id"]) { &data["agent_id"] } else { &data["session_id"] });
    if let Err(miss) = ingest(&format!("{row}\n")) {
        eprintln!("spawn recording boundary miss for lead {lead}: {miss}");
    }
    None
}

fn failure(error: std::io::Error) -> &'static str {
    match error.kind() {
        ErrorKind::NotFound => "FileNotFoundError",
        ErrorKind::PermissionDenied => "PermissionError",
        _ => "OSError",
    }
}

fn ingest(line: &str) -> Result<(), &'static str> {
    match crate::threads::client::spool_ingest(line.as_bytes()) {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(_) => return Err("OSError"),
    }
    let north = std::env::var_os("NORTH_BIN").filter(|path| !path.is_empty()).unwrap_or_else(|| "north".into());
    let mut child = Command::new(north)
        .args(["threads", "ingest"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(failure)?;
    let mut stdin = child.stdin.take().ok_or("OSError")?;
    stdin.write_all(line.as_bytes()).map_err(failure)
}
