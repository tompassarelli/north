use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::calls::{Kind, VerbKinds, classify};
use super::shape::shape;

const SUBJECT_CHARS: usize = 100;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Message {
    pub looks: usize,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Streak {
    pub subject: String,
    pub shapes: Vec<String>,
    pub message: Message,
}

impl Streak {
    pub fn calls(&self) -> usize {
        self.shapes.len()
    }

    pub fn top_shapes(&self, count: usize) -> Vec<(String, usize)> {
        let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
        for shape in &self.shapes {
            *tally.entry(shape).or_default() += 1;
        }
        let mut ranked: Vec<(String, usize)> = tally.into_iter().map(|(shape, calls)| (shape.to_owned(), calls)).collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked.truncate(count);
        ranked
    }
}

pub fn reads_only(kind: Kind) -> bool {
    matches!(kind, Kind::Diagnosis | Kind::Status | Kind::LandingLogs)
}

pub fn is_look(tool: &str, input: &Value, verbs: &VerbKinds) -> bool {
    reads_only(classify(tool, input, verbs))
}

pub fn ends_streak(kind: Kind) -> bool {
    matches!(kind, Kind::Delegate | Kind::ChildMessage | Kind::TomFacing | Kind::Brief)
}

pub struct Call<'a> {
    pub id: &'a str,
    pub tool: &'a str,
    pub input: &'a Value,
}

pub fn read_streaks(jsonl: &str, verbs: &VerbKinds, last: Option<Call>) -> Vec<Streak> {
    let mut streaks = vec![Streak::default()];
    let mut seen = HashSet::new();
    for entry in jsonl.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        if entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        match entry.get("type").and_then(Value::as_str) {
            Some("user") => {
                if let Some(subject) = prompt(&entry) {
                    streaks.push(Streak { subject, ..Streak::default() });
                }
            }
            Some("assistant") => {
                let blocks = entry.pointer("/message/content").and_then(Value::as_array).into_iter().flatten();
                for block in blocks.filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use")) {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    if !id.is_empty() && !seen.insert(id.to_owned()) {
                        continue;
                    }
                    let tool = block.get("name").and_then(Value::as_str).unwrap_or_default();
                    let input = block.get("input").unwrap_or(&Value::Null);
                    record(&mut streaks, tool, input, verbs);
                }
            }
            _ => {}
        }
    }
    if let Some(call) = last.filter(|call| call.id.is_empty() || !seen.contains(call.id)) {
        record(&mut streaks, call.tool, call.input, verbs);
    }
    streaks
}

fn record(streaks: &mut Vec<Streak>, tool: &str, input: &Value, verbs: &VerbKinds) {
    let kind = classify(tool, input, verbs);
    let open = streaks.last_mut().expect("a streak is always open");
    if ends_streak(kind) {
        let (subject, message) = (open.subject.clone(), open.message);
        streaks.push(Streak { subject, shapes: Vec::new(), message });
    } else if reads_only(kind) {
        open.shapes.push(shape(tool, input));
        open.message.looks += 1;
    }
}

fn prompt(entry: &Value) -> Option<String> {
    if entry.get("isMeta").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text = match entry.pointer("/message/content")? {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => {
            if blocks.iter().any(|block| block.get("type").and_then(Value::as_str) == Some("tool_result")) {
                return None;
            }
            blocks.iter().filter_map(|block| block.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")
        }
        _ => return None,
    };
    let line = text.lines().map(str::trim).find(|line| !line.is_empty() && !line.starts_with('<'))?;
    Some(line.chars().take(SUBJECT_CHARS).collect())
}
