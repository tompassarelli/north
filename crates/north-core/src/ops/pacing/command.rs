use super::{grants, process, reservation, sampling, storage};
use serde_json::{Value, json};
use std::io::Read;
use std::path::PathBuf;

pub(super) fn operation(request: &Value) -> Result<Option<Value>, String> {
    let directory = request["directory"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(storage::directory);
    let at = storage::now();
    Ok(Some(match request["op"].as_str().unwrap_or_default() {
        "sample" => sampling::sample(
            request["provider"].as_str(),
            request["initial"].as_i64().unwrap_or(2),
            request["notify"].as_bool().unwrap_or(true),
        )?,
        "reserve" => {
            let path = request["path"]
                .as_str()
                .map(PathBuf::from)
                .unwrap_or_else(|| directory.join("usage-gate.json"));
            json!(reservation::reserve(
                &path,
                request,
                &process::fleet(),
                storage::now
            )?)
        }
        "grant_for" => json!(grants::find(
            &storage::load(&directory.join("pace-control.json")),
            request["account"].as_str().ok_or("account required")?,
            &request["session"],
            &request["task"],
            at,
            &process::fleet()
        )),
        "confirm" => {
            grants::confirm(
                &directory,
                &request["session"],
                &request["tool_use_id"],
                request["launched"].as_bool().unwrap_or(true),
                storage::now,
            )?;
            Value::Null
        }
        "expired_grant_hold" => {
            grants::expired(&directory, request, &process::fleet(), at)?;
            Value::Null
        }
        "grant" => grants::create(
            &directory,
            request,
            &process::fleet(),
            at,
            std::env::var("CODEX_THREAD_ID").ok().as_deref(),
        )?,
        _ => return Ok(None),
    }))
}
pub(super) fn cli(arguments: &[String]) -> Result<(), String> {
    if arguments.first().is_some_and(|s| s == "grant") {
        let request = grant_request(arguments, &sampling::config()?)?;
        println!("{}", operation(&request)?.ok_or("invalid operation")?);
        return Ok(());
    }
    let (provider, initial, notify) = sample_options(arguments)?;
    sampling::sample(provider, initial, notify)?;
    Ok(())
}
fn claude_labels(cfg: &toml_edit::DocumentMut) -> Vec<&str> {
    let claude = cfg.get("accounts").and_then(|accounts| accounts.get("claude"));
    ["profiles", "plans"]
        .iter()
        .filter_map(|key| claude?.get(key)?.as_table_like())
        .flat_map(|table| table.iter().map(|(label, _)| label))
        .collect()
}
pub(super) fn grant_request(arguments: &[String], cfg: &toml_edit::DocumentMut) -> Result<Value, String> {
    let account = arguments.get(1).ok_or("account required")?;
    if !account.strip_prefix("claude:").is_some_and(|label| claude_labels(cfg).contains(&label)) {
        return Err("invalid grant account".into());
    }
    let maximum = arguments
        .get(2)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or("positive worker limit required")?;
    let mut request = json!({"op":"grant","account":account,"max_workers":maximum});
    for pair in arguments[3..].chunks(2) {
        let [flag, value] = pair else {
            return Err("grant options require values".into());
        };
        let key = grant_option_key(flag)?;
        request[key] = if key == "uses" {
            json!(value.parse::<u64>().map_err(|e| e.to_string())?)
        } else {
            json!(value)
        };
    }
    Ok(request)
}
fn grant_option_key(flag: &str) -> Result<&'static str, String> {
    match flag {
        "--until" => Ok("until"),
        "--for" => Ok("lead"),
        "--reason" => Ok("reason"),
        "--task" => Ok("task"),
        "--uses" => Ok("uses"),
        _ => Err(format!("unknown option {flag}")),
    }
}
fn sample_options(arguments: &[String]) -> Result<(Option<&str>, i64, bool), String> {
    let mut provider = None;
    let mut initial = 2;
    let mut notify = true;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--provider" => provider = Some(provider_option(args.next())?),
            "--initial" => initial = initial_option(args.next())?,
            "--dry-run" => notify = false,
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    Ok((provider, initial, notify))
}
fn provider_option(value: Option<&String>) -> Result<&str, String> {
    let value = value.ok_or("provider required")?;
    if !matches!(value.as_str(), "claude" | "gemini") {
        return Err("invalid provider".into());
    }
    Ok(value.as_str())
}
fn initial_option(value: Option<&String>) -> Result<i64, String> {
    value
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .ok_or_else(|| "--initial must be positive".into())
}
pub(super) fn input() -> Result<Value, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    let mut request: Value = serde_json::from_str(&input).map_err(|e| e.to_string())?;
    if request["op"] == "place_spawn" {
        #[derive(serde::Deserialize)]
        struct Gate {
            gate: Box<serde_json::value::RawValue>,
        }
        let raw: Gate = serde_json::from_str(&input).map_err(|e| e.to_string())?;
        request["account_order"] = super::gate_order::accounts(raw.gate.get())?;
    }
    Ok(request)
}
