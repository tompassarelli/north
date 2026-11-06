use super::sampling;
use serde_json::Value;
use std::io::Read;

pub(super) fn operation(request: &Value) -> Result<Option<Value>, String> {
    Ok(Some(match request["op"].as_str().unwrap_or_default() {
        "sample" => sampling::sample(
            request["provider"].as_str(),
            request["initial"].as_i64().unwrap_or(2),
            request["notify"].as_bool().unwrap_or(true),
        )?,
        _ => return Ok(None),
    }))
}
pub(super) fn cli(arguments: &[String]) -> Result<(), String> {
    let (provider, initial, notify) = sample_options(arguments)?;
    sampling::sample(provider, initial, notify)?;
    Ok(())
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
    serde_json::from_str(&input).map_err(|e| e.to_string())
}
