use fancy_regex::Regex;
use serde_json::Value;

use super::events::{final_text, tool_calls};
use super::json::Json;
use super::pyfmt::{lstrip, splitlines, strip};
use crate::error::NorthError;

const COMMAND_TIMEOUT_S: f64 = 120.0;

fn fenced_blocks(text: &str) -> Vec<(&str, Vec<&str>)> {
    let mut blocks = Vec::new();
    let (mut prose, mut body): (&str, Option<Vec<&str>>) = ("", None);
    for line in splitlines(text, false) {
        if lstrip(line).starts_with("```") {
            match body.take() {
                None => body = Some(Vec::new()),
                Some(done) => {
                    blocks.push((prose, done));
                    prose = "";
                }
            }
        } else if let Some(open) = body.as_mut() {
            open.push(line);
        } else if !strip(line).is_empty() {
            prose = line;
        }
    }
    blocks
}

fn found(pattern: &Regex, text: &str) -> Result<bool, NorthError> {
    pattern.is_match(text).map_err(|error| NorthError::Usage(format!("policy-replay: detector pattern failed on a run: {error}")))
}

fn block_offers(pattern: &Regex, warning: Option<&Regex>, prose: &str, body: &[&str]) -> Result<bool, NorthError> {
    let mut warned = false;
    if let Some(warning) = warning {
        warned = found(warning, prose)?;
        for comment in body.iter().filter(|line| strip(line).starts_with('#')) {
            warned = warned || found(warning, comment)?;
        }
    }
    for line in body {
        let (command, trailing) = line.split_once(" #").unwrap_or((line, ""));
        if strip(line).starts_with('#') || !found(pattern, command)? {
            continue;
        }
        let trailing_warned = match warning {
            Some(warning) => found(warning, trailing)?,
            None => false,
        };
        if !warned && !trailing_warned {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn offered_command(pattern: &Regex, warning: Option<&Regex>, text: &str) -> Result<bool, NorthError> {
    for (prose, body) in fenced_blocks(text) {
        if block_offers(pattern, warning, prose, &body)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn compile(pattern: &str, ignore_case: bool) -> Result<Regex, NorthError> {
    let source = if ignore_case { format!("(?i){pattern}") } else { pattern.to_owned() };
    Regex::new(&source).map_err(|error| NorthError::Usage(format!("policy-replay: bad detector pattern {pattern:?}: {error}")))
}

fn first_line(text: &str) -> &str {
    splitlines(text, false).into_iter().find(|line| !strip(line).is_empty()).unwrap_or("")
}

fn tool_called(detector: &Value, evs: &[Json], pattern: &Regex) -> Result<bool, NorthError> {
    let tool = detector.get("tool").and_then(Value::as_str);
    for call in tool_calls(evs) {
        if call.text("name") == tool && found(pattern, &dumped_input(call))? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn detect(detector: &Value, evs: &[Json], run_command: &mut dyn FnMut(&str, f64) -> bool) -> Result<bool, NorthError> {
    let text = |key: &str| detector.get(key).and_then(Value::as_str);
    let ignore_case = detector.get("ignore_case").and_then(Value::as_bool).unwrap_or(false);
    let pattern = text("pattern").unwrap_or("");
    let matched = match text("kind").unwrap_or("") {
        "command" => {
            let timeout = detector.get("timeout_s").and_then(Value::as_f64).unwrap_or(COMMAND_TIMEOUT_S);
            run_command(text("command").unwrap_or(""), timeout)
        }
        "regex" => {
            let answer = final_text(evs);
            let answer = if text("scope") == Some("first_line") { first_line(answer) } else { answer };
            found(&compile(pattern, ignore_case)?, answer)?
        }
        "code_command" => {
            let warning = text("warning").filter(|warning| !warning.is_empty()).map(|warning| compile(warning, true)).transpose()?;
            offered_command(&compile(pattern, ignore_case)?, warning.as_ref(), final_text(evs))?
        }
        "tool_call" => tool_called(detector, evs, &compile(pattern, ignore_case)?)?,
        kind => return Err(NorthError::Usage(format!("policy-replay: unknown detector kind {kind}"))),
    };
    Ok(matched == detector.get("expect").and_then(Value::as_bool).unwrap_or(true))
}

fn dumped_input(call: &Json) -> String {
    match call.field("input") {
        Json::Null if call.entries().iter().all(|(key, _)| key != "input") => "{}".to_owned(),
        input => input.dumps(),
    }
}
