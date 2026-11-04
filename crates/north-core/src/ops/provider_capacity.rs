//! `north provider-capacity events|status [--minutes N]`: Codex turns that failed because OpenAI had no capacity for the model, read from Codex rollouts.
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::foundation::clock::Timestamp;

#[cfg(test)]
mod tests;

const ERROR_INFO: &str = "server_overloaded";
const MESSAGE: &str = "Selected model is at capacity";
const USAGE: &str = "usage: north provider-capacity events|status [--minutes N]";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Outcome {
    pub at: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub kind: &'static str,
    pub rollout: String,
}

fn stamp(text: &str) -> Option<f64> {
    Timestamp::parse_rfc3339(text).map(Timestamp::unix_seconds_f64)
}

fn at_capacity(payload: &Value) -> bool {
    let error = &payload["error"];
    error["codex_error_info"].as_str() == Some(ERROR_INFO) || error["message"].as_str().is_some_and(|message| message.contains(MESSAGE))
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(members) => !members.is_empty(),
    }
}

fn present(value: &Value) -> Option<String> {
    value.as_str().filter(|text| !text.is_empty()).map(str::to_owned)
}

fn outcomes(paths: &[String], since: f64) -> Vec<Outcome> {
    let mut found = Vec::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(path) else { continue };
        let (mut model, mut effort) = (None, None);
        for record in text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
            let payload = &record["payload"];
            match record["type"].as_str() {
                Some("turn_context") => {
                    model = present(&payload["model"]).or(model);
                    effort = present(&payload["effort"]).or(effort);
                }
                Some("event_msg") if payload["type"].as_str() == Some("task_complete") => {
                    let kind = if at_capacity(payload) {
                        "capacity"
                    } else if truthy(&payload["error"]) {
                        "error"
                    } else {
                        "ok"
                    };
                    let Some(at) = record["timestamp"].as_str().filter(|at| stamp(at).is_some_and(|moment| moment >= since)) else { continue };
                    found.push(Outcome { at: at.to_owned(), model: model.clone(), effort: effort.clone(), kind, rollout: path.clone() });
                }
                _ => {}
            }
        }
    }
    found.sort_by(|left, right| stamp(&left.at).partial_cmp(&stamp(&right.at)).unwrap_or(std::cmp::Ordering::Equal));
    found
}

fn modified_since(path: &Path, since: f64) -> bool {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|time| time.duration_since(UNIX_EPOCH).is_ok_and(|age| age.as_secs_f64() >= since))
}

fn walk(directory: &Path, since: f64, found: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            walk(&path, since, found);
        } else if path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl")) && modified_since(&path, since) {
            found.push(path.to_string_lossy().into_owned());
        }
    }
}

fn rollouts(root: &Path, since: f64) -> Result<Vec<String>, String> {
    let state = root.join("sqlite/state_5.sqlite");
    if state.exists() {
        let db = Connection::open_with_flags(&state, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI).map_err(|error| error.to_string())?;
        let mut statement = db.prepare("SELECT rollout_path FROM threads WHERE updated_at >= ?").map_err(|error| error.to_string())?;
        return statement.query_map([since as i64], |row| row.get::<_, String>(0)).and_then(Iterator::collect).map_err(|error| error.to_string());
    }
    let mut found = Vec::new();
    walk(&root.join("sessions"), since, &mut found);
    Ok(found)
}

fn status(rows: &[Outcome]) -> Vec<(Option<String>, String)> {
    let mut open: Vec<(Option<String>, String)> = Vec::new();
    for row in rows {
        match row.kind {
            "capacity" if !open.iter().any(|(model, _)| model == &row.model) => open.push((row.model.clone(), row.at.clone())),
            "ok" => open.retain(|(model, _)| model != &row.model),
            _ => {}
        }
    }
    let mut earliest: Option<&(Option<String>, String)> = None;
    for entry in &open {
        if earliest.is_none_or(|best| stamp(&entry.1) < stamp(&best.1)) {
            earliest = Some(entry);
        }
    }
    earliest.into_iter().cloned().collect()
}

fn text(value: &str) -> String {
    let mut out = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            plain if plain.is_ascii() && !plain.is_ascii_control() => out.push(plain),
            other => {
                let mut units = [0u16; 2];
                for unit in other.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out + "\""
}

fn optional(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "null".to_owned(), text)
}

pub(crate) fn events_json(rows: &[Outcome]) -> String {
    let items: Vec<String> = rows
        .iter()
        .filter(|row| row.kind == "capacity")
        .map(|row| format!("{{\"at\": {}, \"model\": {}, \"effort\": {}, \"outcome\": {}, \"rollout\": {}}}", text(&row.at), optional(&row.model), optional(&row.effort), text(row.kind), text(&row.rollout)))
        .collect();
    format!("[{}]", items.join(", "))
}

pub(crate) fn status_json(rows: &[Outcome]) -> String {
    match status(rows).first() {
        Some((model, since)) => format!("{{\"codex\": {{\"since\": {}, \"model\": {}}}}}", text(since), optional(model)),
        None => "{}".to_owned(),
    }
}

fn root() -> PathBuf {
    match std::env::var_os("CODEX_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north-data/codex-pooled"),
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(verb) = arguments.first().filter(|verb| matches!(verb.as_str(), "events" | "status")) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let minutes = match arguments.iter().position(|argument| argument == "--minutes") {
        Some(at) => match arguments.get(at + 1).and_then(|value| value.parse::<f64>().ok()) {
            Some(minutes) => minutes,
            None => {
                eprintln!("{USAGE}");
                return 2;
            }
        },
        None => 15.0,
    };
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |age| age.as_secs_f64());
    let since = now - minutes * 60.0;
    let rows = match rollouts(&root(), since) {
        Ok(paths) => outcomes(&paths, since),
        Err(error) => {
            eprintln!("provider-capacity: {error}");
            return 1;
        }
    };
    println!("{}", if verb == "events" { events_json(&rows) } else { status_json(&rows) });
    0
}
