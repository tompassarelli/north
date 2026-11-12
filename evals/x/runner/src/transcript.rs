use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{Value, json};

const SHOWN: usize = 300;
const POLICY_LINE: usize = 60;
const NOT_APPLIED: &str = "not applied: container shell";

#[derive(Debug, Default)]
pub struct Reading {
    pub efforts: Vec<String>,
    pub rate_limited: bool,
    pub requests: usize,
    pub guards: Vec<Value>,
    first_request: String,
    replied: bool,
    usage: BTreeMap<String, Value>,
    calls: BTreeMap<String, Value>,
}

impl Reading {
    pub fn effort(&self) -> Option<String> {
        (!self.efforts.is_empty()).then(|| self.efforts.join("+"))
    }

    fn tokens(&self, key: &str) -> u64 {
        self.usage
            .values()
            .filter_map(|usage| usage.get(key).and_then(Value::as_u64))
            .sum()
    }

    pub fn instruction_files(&self) -> Vec<String> {
        let mut files: Vec<String> = self
            .first_request
            .lines()
            .filter_map(|line| line.trim().strip_prefix("Contents of "))
            .filter_map(|rest| rest.split_once(" (").map(|(path, _)| path.to_owned()))
            .collect();
        files.sort();
        files.dedup();
        files
    }

    pub fn policy_lines_found(&self, policy: &str) -> (usize, usize) {
        let lines: Vec<&str> = policy
            .lines()
            .map(str::trim)
            .filter(|line| line.chars().count() >= POLICY_LINE)
            .collect();
        let found = lines
            .iter()
            .filter(|line| self.first_request.contains(*line))
            .count();
        (lines.len(), found)
    }

    pub fn first_request_bytes(&self) -> usize {
        self.first_request.len()
    }

    pub fn row(&self) -> Value {
        json!({
            "effort": self.effort(),
            "rate_limited": self.rate_limited,
            "requests": self.requests,
            "tokens": {
                "input": self.tokens("input_tokens"),
                "output": self.tokens("output_tokens"),
                "cache_creation": self.tokens("cache_creation_input_tokens"),
                "cache_read": self.tokens("cache_read_input_tokens"),
            },
            "guards": self.guards,
        })
    }

    pub fn not_applied(mut self, records: &str) -> Self {
        for record in records
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        {
            let field = |key: &str| shown(record[key].as_str().unwrap_or_default());
            self.guards.push(json!({
                "kind": NOT_APPLIED,
                "at": record.get("at"),
                "call": {"tool": record.get("tool"), "input": field("command")},
                "said": field("said"),
            }));
        }
        self
    }

    fn guard(&mut self, kind: &str, call: Option<&str>, text: &str, frame: &Value) {
        self.guards.push(json!({
            "kind": kind,
            "at": frame.get("timestamp"),
            "call": call.and_then(|id| self.calls.get(id)),
            "said": shown(text),
        }));
    }

    fn request(&mut self, frame: &Value) {
        self.requests += 1;
        let effort = frame
            .pointer("/params/output_config/effort")
            .and_then(Value::as_str);
        if let Some(effort) =
            effort.filter(|effort| !self.efforts.iter().any(|known| known == effort))
        {
            self.efforts.push(effort.to_owned());
        }
    }

    fn first(&mut self, content: &Value) {
        if !self.replied {
            self.first_request.push_str(&text(content));
            self.first_request.push('\n');
        }
    }

    fn assistant(&mut self, frame: &Value) {
        self.replied = true;
        if frame.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true) {
            self.rate_limited |= frame.get("error").and_then(Value::as_str) == Some("rate_limit");
            return;
        }
        let message = &frame["message"];
        if let (Some(id), Some(usage)) = (
            message.get("id").and_then(Value::as_str),
            message.get("usage"),
        ) {
            self.usage.insert(id.to_owned(), usage.clone());
        }
        for block in blocks(message).filter(|block| block["type"] == "tool_use") {
            if let Some(id) = block.get("id").and_then(Value::as_str) {
                let call =
                    json!({"tool": block.get("name"), "input": shown(&block["input"].to_string())});
                self.calls.insert(id.to_owned(), call);
            }
        }
    }

    fn user(&mut self, frame: &Value) {
        let refused: Vec<(Option<String>, String)> = blocks(&frame["message"])
            .filter(|block| block["type"] == "tool_result" && block["is_error"] == true)
            .map(|block| {
                (
                    block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    text(&block["content"]),
                )
            })
            .filter(|(_, said)| said.starts_with("PreToolUse:"))
            .collect();
        for (call, said) in refused {
            self.guard("refused", call.as_deref(), &said, frame);
        }
    }

    fn attachment(&mut self, frame: &Value) {
        let attachment = &frame["attachment"];
        if attachment["type"] != "hook_additional_context"
            || attachment["hookEvent"] != "PreToolUse"
        {
            return;
        }
        let call = attachment.get("toolUseID").and_then(Value::as_str);
        self.guard("changed", call, &text(&attachment["content"]), frame);
    }
}

fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
    message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|part| part.get("text").map_or_else(|| text(part), text))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn shown(text: &str) -> String {
    text.chars().take(SHOWN).collect()
}

pub fn parse(transcript: &str) -> Reading {
    let mut reading = Reading::default();
    for frame in transcript
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    {
        match frame.get("type").and_then(Value::as_str) {
            Some("api-request") => reading.request(&frame),
            Some("api-request-shape") => reading.first(&frame["shape"]["system"]),
            Some("api-request-blob") => reading.first(&frame["message"]["content"]),
            Some("assistant") => reading.assistant(&frame),
            Some("user") => reading.user(&frame),
            Some("attachment") => reading.attachment(&frame),
            _ => {}
        }
    }
    reading
}

pub fn read(transcript: &Path, stood_down: &Path) -> Reading {
    parse(&fs::read_to_string(transcript).unwrap_or_default())
        .not_applied(&fs::read_to_string(stood_down).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: &str = "# Policy\n- Keep working until the goal is done or a blocker waits on the owner's decision.\n";
    const TRANSCRIPT: &str = r#"{"type":"api-request-shape","shape":{"system":[{"type":"text","text":"You are Claude Code."}],"tools":[]}}
{"type":"api-request-blob","message":{"role":"user","content":[{"type":"text","text":"Contents of /home/owner/.claude/CLAUDE.md (user instructions):\n\n- Keep working until the goal is done or a blocker waits on the owner's decision."}]}}
{"type":"api-request","params":{"model":"claude-opus-5-5","output_config":{"effort":"medium"}}}
{"type":"assistant","message":{"id":"msg_1","usage":{"input_tokens":2,"output_tokens":10,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000},"content":[{"type":"text","text":"Running it."}]}}
{"type":"assistant","message":{"id":"msg_1","usage":{"input_tokens":2,"output_tokens":40,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000},"content":[{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"grep -r x ."}}]}}
{"type":"user","timestamp":"2026-11-12T12:00:02Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","is_error":true,"content":"PreToolUse:Bash hook error: BLOCKED: recursive grep"}]}}
{"type":"api-request","params":{"output_config":{"effort":"medium"}}}
{"type":"assistant","isApiErrorMessage":true,"error":"rate_limit","message":{"id":"msg_2","usage":{"input_tokens":0,"output_tokens":0},"content":[{"type":"text","text":"You've hit your weekly limit"}]}}
"#;

    #[test]
    fn a_claude_code_transcript_gives_effort_tokens_rate_limit_guards_and_policy_lines() {
        let reading = parse(TRANSCRIPT);
        let row = reading.row();
        assert_eq!(row["effort"], "medium");
        assert_eq!(row["requests"], 2);
        assert_eq!(row["rate_limited"], true);
        assert_eq!(
            row["tokens"],
            json!({"input": 2, "output": 40, "cache_creation": 100, "cache_read": 1000})
        );
        assert_eq!(row["guards"].as_array().map(Vec::len), Some(1));
        assert_eq!(row["guards"][0]["kind"], "refused");
        assert_eq!(row["guards"][0]["call"]["tool"], "Bash");
        assert_eq!(
            reading.instruction_files(),
            ["/home/owner/.claude/CLAUDE.md"]
        );
        assert_eq!(reading.policy_lines_found(POLICY), (1, 1));
        assert_eq!(parse("").policy_lines_found(POLICY), (1, 0));
        let stood_down = r#"{"at":"2026-11-12T13:00:00Z","tool":"Bash","command":"find john -iname x","said":"modern-search-guard: rewrote a find tree search to fd"}"#;
        let guards = parse(TRANSCRIPT).not_applied(stood_down).row()["guards"].clone();
        assert_eq!(guards[1]["kind"], "not applied: container shell");
        assert_eq!(guards[1]["call"]["input"], "find john -iname x");
    }
}
