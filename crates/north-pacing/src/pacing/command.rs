use super::sampling;
use serde_json::Value;
use std::io::Read;

pub(super) fn operation(request: &Value) -> Result<Option<Value>, String> {
    Ok(Some(match request["op"].as_str().unwrap_or_default() {
        "sample" => sampling::sample(request["provider"].as_str())?,
        _ => return Ok(None),
    }))
}
pub(super) fn cli(arguments: &[String]) -> Result<(), String> {
    sampling::sample(sample_options(arguments)?)?;
    Ok(())
}
fn sample_options(arguments: &[String]) -> Result<Option<&str>, String> {
    let mut provider = None;
    let mut args = arguments.iter();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--provider" => provider = Some(provider_option(args.next())?),
            _ => return Err(format!("unknown option {argument}")),
        }
    }
    Ok(provider)
}
fn provider_option(value: Option<&String>) -> Result<&str, String> {
    let value = value.ok_or("provider required")?;
    if !matches!(value.as_str(), "claude" | "gemini") {
        return Err("invalid provider".into());
    }
    Ok(value.as_str())
}
pub(super) fn input() -> Result<Value, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&input).map_err(|e| e.to_string())
}
