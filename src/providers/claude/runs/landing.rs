use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use super::truthy;
use crate::foundation::clock::Timestamp;

static LAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[;&|(\n])\s*(?:\w+=\S*\s+)*(?:(?:sudo|time|nohup|command|timeout\s+\S+)\s+)?(?:\S*/)?(?:safe-push|git(?:\s+-C\s+\S+)?\s+push)\b").unwrap()
});
static COMMIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bgit\b[^|;&\n]*?\scommit\b|(?:^|[;&|(\n])\s*\$\{?\w+\}?\s+commit\b").unwrap());
static SUBJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\s-[a-zA-Z]*m[ =]?\s*(?:"\$\(cat <<-?['"]?\w+['"]?\s*\n)?["']?([^"'\n]+)"#).unwrap()
});
static HEREDOC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\s-[a-zA-Z]*F\s*-\s*<<-?['"]?\w+['"]?[^\n]*\n([^\n]+)"#).unwrap());
static LANE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^([0-9a-f]{7,40}) (.+)$").unwrap());
static MADE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\[[^\]\s]+(?: \([^)]*\))? ([0-9a-f]{7,40})\] (.+)$").unwrap());
static PUSHED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^safe-push: landed ([0-9a-f]{40})\b").unwrap());
static ISSUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:tompassarelli/)?([a-z][\w.-]*#\d+)\b").unwrap());
static GH_ISSUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgh issue (?:view|close|comment|edit|reopen) (\d+)\b.*?(?:-R|--repo)[ =](?:[\w-]+/)?([\w.-]+)").unwrap()
});
static THREADS_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:\S+/)?threads (?:show|claim|release) ").unwrap());

pub(super) struct MainIndex {
    shas: Vec<(String, String)>,
    subjects: HashMap<String, Vec<(String, String)>>,
}

impl MainIndex {
    pub(super) fn parse(text: &str) -> Self {
        let mut shas: Vec<(String, String)> = Vec::new();
        let mut positions: HashMap<String, usize> = HashMap::new();
        let mut subjects: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for line in text.lines() {
            let mut parts = line.splitn(3, ' ');
            let (sha, date, subject) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            match positions.get(sha) {
                Some(&position) => shas[position].1 = date.to_owned(),
                None => {
                    positions.insert(sha.to_owned(), shas.len());
                    shas.push((sha.to_owned(), date.to_owned()));
                }
            }
            subjects.entry(subject.trim().to_owned()).or_default().push((sha.to_owned(), date.to_owned()));
        }
        Self { shas, subjects }
    }
}

pub(super) struct Step {
    command: Option<String>,
    code: Option<i64>,
    output: Option<String>,
    finished: Option<String>,
}

pub(super) fn utc(text: Option<&str>) -> Option<String> {
    text.filter(|text| !text.is_empty()).and_then(|text| Timestamp::parse(text).ok()).map(Timestamp::iso)
}

pub(super) fn checks(records: &[Value], main: &MainIndex) -> Map<String, Value> {
    let mut calls: HashMap<String, usize> = HashMap::new();
    let mut steps: Vec<Step> = Vec::new();
    let (mut staffed, mut pending, mut review): (Option<&str>, Option<&str>, Option<&str>) = (None, None, None);
    for record in records {
        staffed = staffed.or_else(|| record["timestamp"].as_str().filter(|at| !at.is_empty()));
        let content = &record["message"]["content"];
        let texted = |parts: &Value| parts.as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "text"));
        if record["type"] == "user" && !truthy(&record["isMeta"]) && (content.is_string() || texted(content)) {
            review = review.or(pending);
        }
        let Some(parts) = content.as_array() else { continue };
        if record["type"] == "assistant" {
            if parts.iter().any(|part| part["type"] == "tool_use") {
                pending = None;
            } else if texted(content) {
                pending = record["timestamp"].as_str();
            }
        }
        for part in parts {
            let id = part["id"].to_string();
            if part["type"] == "tool_use" {
                let command = (part["name"] == "Bash").then(|| part["input"]["command"].as_str().map(str::to_owned)).flatten();
                calls.insert(id, steps.len());
                steps.push(Step { command, code: None, output: None, finished: None });
            } else if part["type"] == "tool_result"
                && let Some(&index) = calls.get(&part["tool_use_id"].to_string())
            {
                let step = &mut steps[index];
                step.code = Some(i64::from(truthy(&part["is_error"])));
                if step.output.is_none() {
                    step.output = Some(part["content"].as_str().map_or_else(|| python_dumps(&part["content"]), str::to_owned));
                    step.finished = record["timestamp"].as_str().map(str::to_owned);
                }
            }
        }
    }
    let mut out = tally(&steps, main);
    out.insert("staffed".into(), json!(utc(staffed)));
    out.insert("review_requested".into(), json!(utc(review.or(pending))));
    out
}

fn tally(steps: &[Step], main: &MainIndex) -> Map<String, Value> {
    let (mut pushed, mut committed, mut first_commit) = (false, false, None);
    let (mut made_shas, mut made_subjects, mut refs) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
    for step in steps {
        let Some(command) = step.command.as_deref().filter(|command| !command.is_empty()) else { continue };
        let succeeded = step.code == Some(0);
        pushed |= succeeded && LAND.is_match(command);
        if let Some(found) = COMMIT.find(command).filter(|_| step.code.unwrap_or(0) == 0) {
            committed = true;
            first_commit = first_commit.or(step.finished.as_deref());
            refs.extend(ISSUE.captures_iter(&command[found.end()..]).map(|c| c[1].to_owned()));
            if let Some(subject) = SUBJECT.captures_at(command, found.end()).or_else(|| HEREDOC.captures_at(command, found.end())) {
                made_subjects.push(subject[1].trim().to_owned());
            }
            for made in MADE.captures_iter(step.output.as_deref().unwrap_or("")) {
                made_shas.push(made[1].to_owned());
                made_subjects.push(made[2].trim().to_owned());
            }
        } else if committed && succeeded && command.contains("origin/main..") && let Some(output) = &step.output {
            made_subjects.extend(LANE.captures_iter(output).map(|lane| lane[2].trim().to_owned()));
        } else if succeeded && THREADS_REF.is_match(command) {
            refs.extend(ISSUE.captures_iter(command).map(|c| c[1].to_owned()));
        }
        if committed && succeeded {
            made_shas.extend(PUSHED.captures_iter(step.output.as_deref().unwrap_or("")).map(|landed| landed[1].to_owned()));
        }
        if succeeded {
            refs.extend(GH_ISSUE.captures_iter(command).map(|c| format!("{}#{}", &c[2], &c[1])));
        }
    }
    landing(main, &made_shas, &made_subjects, first_commit, committed || pushed, refs)
}

fn landing(main: &MainIndex, made_shas: &[String], made_subjects: &[String], first_commit: Option<&str>, expects: bool, refs: Vec<String>) -> Map<String, Value> {
    let mut seen_subjects: Vec<&String> = Vec::new();
    for subject in made_subjects {
        if !seen_subjects.contains(&subject) {
            seen_subjects.push(subject);
        }
    }
    let mut on_main: Vec<(String, Option<String>)> = main
        .shas
        .iter()
        .filter(|(sha, _)| made_shas.iter().any(|prefix| sha.starts_with(prefix.as_str())))
        .map(|(sha, date)| (sha.clone(), utc(Some(date))))
        .collect();
    for subject in seen_subjects {
        on_main.extend(main.subjects.get(subject).into_iter().flatten().map(|(sha, date)| (sha.clone(), utc(Some(date)))));
    }
    if let Some(first) = first_commit {
        let day: String = utc(Some(first)).unwrap_or_default().chars().take(11).collect();
        on_main.retain(|(_, date)| date.as_deref().is_some_and(|date| date >= day.as_str()));
    }
    let landing = on_main.iter().min_by(|left, right| (&left.1, &left.0).cmp(&(&right.1, &right.0)));
    let mut ranked: Vec<&String> = Vec::new();
    for reference in &refs {
        if !ranked.contains(&reference) {
            ranked.push(reference);
        }
    }
    let count = |reference: &String| refs.iter().filter(|other| *other == reference).count();
    ranked.sort_by_key(|reference| std::cmp::Reverse(count(reference)));
    let mut out = Map::new();
    out.insert("landed".into(), json!(i32::from(landing.is_some())));
    out.insert("expects_landing".into(), json!(i32::from(expects)));
    out.insert("landed_sha".into(), json!(landing.map(|(sha, _)| sha)));
    out.insert("landed_at".into(), json!(landing.and_then(|(_, date)| date.as_ref())));
    out.insert("refs".into(), json!(ranked));
    out
}

fn python_dumps(value: &Value) -> String {
    match value {
        Value::String(text) => {
            let mut out = String::from("\"");
            for unit in text.encode_utf16() {
                match unit {
                    0x22 => out.push_str("\\\""),
                    0x5c => out.push_str("\\\\"),
                    0x0a => out.push_str("\\n"),
                    0x0d => out.push_str("\\r"),
                    0x09 => out.push_str("\\t"),
                    0x08 => out.push_str("\\b"),
                    0x0c => out.push_str("\\f"),
                    0x20..=0x7e => out.push(unit as u8 as char),
                    _ => out.push_str(&format!("\\u{unit:04x}")),
                }
            }
            out.push('"');
            out
        }
        Value::Array(items) => format!("[{}]", items.iter().map(python_dumps).collect::<Vec<_>>().join(", ")),
        Value::Object(entries) => format!(
            "{{{}}}",
            entries.iter().map(|(key, item)| format!("{}: {}", python_dumps(&Value::from(key.as_str())), python_dumps(item))).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}
