use std::collections::HashMap;

use serde_json::{Value, json};

use super::hook_rules::classify;

const SLEEP_BLOCK: &str = "<tool_use_error>Blocked:";
const HOOK_ERROR: &str = " hook error: ";

pub fn mentions_hook(line: &str) -> bool {
    line.contains("hook error: ") || line.contains(SLEEP_BLOCK) || line.contains("\"hook_success\"") || line.contains("\"hook_additional_context\"") || line.contains("\"type\":\"assistant\"")
}

struct Call {
    tool: String,
    turn: usize,
}

struct Scan<'a> {
    session: &'a str,
    calls: HashMap<String, Call>,
    rewrites: HashMap<String, &'static str>,
    rows: Vec<Value>,
}

pub fn events(session: &str, lines: &[Value]) -> Vec<Value> {
    let mut scan = Scan { session, calls: HashMap::new(), rewrites: HashMap::new(), rows: Vec::new() };
    for line in lines {
        match line["type"].as_str() {
            Some("assistant") => scan.calls(line),
            Some("user") => scan.refusals(line),
            Some("attachment") => scan.attachment(line),
            _ => {}
        }
    }
    let Scan { rows, rewrites, .. } = scan;
    rows.into_iter().filter(|row| row["verdict"] != "advisory" || row["tool_use"].as_str().and_then(|id| rewrites.get(id)).is_none_or(|rule| row["rule"] != *rule)).collect()
}

fn refusal_text(part: &Value) -> Option<String> {
    let text = match &part["content"] {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().filter_map(|item| item["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => return None,
    };
    Some(text)
}

fn hook_event(text: &str) -> Option<(&'static str, String)> {
    if text.starts_with(SLEEP_BLOCK) {
        return Some(("PreToolUse", text.to_owned()));
    }
    let (head, reason) = text.split_once(HOOK_ERROR)?;
    let event = ["PreToolUse", "PostToolUse"].into_iter().find(|event| head.starts_with(event) && !head.contains(' '))?;
    let reason = reason.strip_prefix('[').and_then(|rest| rest.split_once("]: ")).map_or(reason, |(_, reason)| reason);
    Some((event, reason.to_owned()))
}

impl Scan<'_> {
    fn calls(&mut self, line: &Value) {
        for part in line["message"]["content"].as_array().into_iter().flatten().filter(|part| part["type"] == "tool_use") {
            let Some(id) = part["id"].as_str() else { continue };
            let turn = self.calls.len() + 1;
            self.calls.entry(id.to_owned()).or_insert_with(|| Call { tool: part["name"].as_str().unwrap_or("").to_owned(), turn });
        }
    }

    fn push(&mut self, tool_use: &str, tool: Option<&str>, event: &str, verdict: &str, rule: &str, at: &Value) {
        let call = self.calls.get(tool_use);
        let tool = call.map(|call| call.tool.as_str()).filter(|tool| !tool.is_empty()).or(tool).unwrap_or("");
        let turn = call.map_or(0, |call| call.turn);
        self.rows.push(json!({"type": "hook", "session": self.session, "tool_use": tool_use, "tool": tool, "event": event, "verdict": verdict, "rule": rule, "at": at, "turn": turn}));
    }

    fn refusals(&mut self, line: &Value) {
        for part in line["message"]["content"].as_array().into_iter().flatten() {
            if part["type"] != "tool_result" || part["is_error"] != true {
                continue;
            }
            let (Some(id), Some(text)) = (part["tool_use_id"].as_str(), refusal_text(part)) else { continue };
            let Some((event, reason)) = hook_event(&text) else { continue };
            let rule = if text.starts_with(SLEEP_BLOCK) { "claude-sleep" } else { classify(&reason) };
            self.push(id, None, event, "refusal", rule, &line["timestamp"]);
        }
    }

    fn attachment(&mut self, line: &Value) {
        let attachment = &line["attachment"];
        let event = attachment["hookEvent"].as_str().unwrap_or("");
        if !matches!(event, "PreToolUse" | "PostToolUse") {
            return;
        }
        let Some(id) = attachment["toolUseID"].as_str() else { return };
        let tool = attachment["hookName"].as_str().and_then(|name| name.split_once(':')).map(|(_, tool)| tool);
        match attachment["type"].as_str() {
            Some("hook_success") => {
                let output: Value = attachment["stdout"].as_str().and_then(|text| serde_json::from_str(text.trim()).ok()).unwrap_or_default();
                let specific = &output["hookSpecificOutput"];
                if specific["updatedInput"].is_null() {
                    return;
                }
                let rule = classify(specific["additionalContext"].as_str().unwrap_or(""));
                self.rewrites.insert(id.to_owned(), rule);
                self.push(id, tool, event, "rewrite", rule, &line["timestamp"]);
            }
            Some("hook_additional_context") => {
                let texts: Vec<&str> = match &attachment["content"] {
                    Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
                    Value::String(text) => vec![text.as_str()],
                    _ => Vec::new(),
                };
                for text in texts {
                    self.push(id, tool, event, "advisory", classify(text), &line["timestamp"]);
                }
            }
            _ => {}
        }
    }
}
