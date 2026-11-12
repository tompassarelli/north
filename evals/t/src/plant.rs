use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::{Value, json};

pub const CLAUDE: &str = "2.1.322";
pub const MODEL: &str = "claude-opus-5-5";
const STAMP: &str = "2026-01-01T00:00:00.000Z";

fn digest(parts: &[&str]) -> String {
    let mut first = DefaultHasher::new();
    parts.hash(&mut first);
    let mut second = DefaultHasher::new();
    (parts, 1u8).hash(&mut second);
    format!("{:016x}{:016x}", first.finish(), second.finish())
}

fn uuid(session: &str, index: usize) -> String {
    let hex = digest(&[session, &index.to_string()]);
    format!("{}-{}-4{}-8{}-{}", &hex[..8], &hex[8..12], &hex[13..16], &hex[17..20], &hex[20..32])
}

struct Chain<'a> {
    session: &'a str,
    cwd: &'a str,
    parent: Option<String>,
    lines: Vec<String>,
}

impl Chain<'_> {
    fn record(&mut self, kind: &str, message: Value) {
        let uuid = uuid(self.session, self.lines.len());
        let mut line = json!({
            "parentUuid": self.parent,
            "isSidechain": false,
            "type": kind,
            "message": message,
            "uuid": uuid,
            "timestamp": STAMP,
            "userType": "external",
            "entrypoint": "sdk-cli",
            "cwd": self.cwd,
            "sessionId": self.session,
            "version": CLAUDE,
            "gitBranch": "main",
        });
        if kind == "assistant" {
            line["requestId"] = json!(format!("req_{}", &digest(&[self.session, &uuid])[..24]));
        }
        self.parent = Some(uuid);
        self.lines.push(line.to_string());
    }

    fn turn(&mut self, index: usize, turn: &Value) -> Result<(), String> {
        let text = turn["text"].as_str().unwrap_or_default();
        match turn["role"].as_str() {
            Some("user") => self.record("user", json!({"role": "user", "content": text})),
            Some("assistant") => self.assistant_turn(index, text, turn["toolCalls"].as_array().map_or(&[][..], Vec::as_slice)),
            _ => return Err(format!("prior turn {index} has no role of user or assistant")),
        }
        Ok(())
    }

    fn assistant_turn(&mut self, index: usize, text: &str, calls: &[Value]) {
        let ids: Vec<String> = (0..calls.len()).map(|call| format!("toolu_{}", &digest(&[self.session, &index.to_string(), &call.to_string()])[..24])).collect();
        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(json!({"type": "text", "text": text}));
        }
        blocks.extend(calls.iter().zip(&ids).map(|(call, id)| json!({"type": "tool_use", "id": id, "name": call["tool"], "input": call["input"]})));
        let stop = if calls.is_empty() { "end_turn" } else { "tool_use" };
        self.record("assistant", assistant(self.session, blocks, stop));
        if calls.is_empty() {
            return;
        }
        let results: Vec<Value> = calls.iter().zip(&ids).map(|(call, id)| json!({"type": "tool_result", "tool_use_id": id, "content": call["result"].as_str().unwrap_or_default()})).collect();
        self.record("user", json!({"role": "user", "content": results}));
    }
}

pub fn transcript(turns: &[Value], session: &str, cwd: &str) -> Result<String, String> {
    let mut chain = Chain { session, cwd, parent: None, lines: Vec::new() };
    for (index, turn) in turns.iter().enumerate() {
        chain.turn(index, turn)?;
    }
    Ok(chain.lines.join("\n") + "\n")
}

fn assistant(session: &str, content: Vec<Value>, stop: &str) -> Value {
    json!({
        "id": format!("msg_{}", &digest(&[session, "message", &content.len().to_string()])[..24]),
        "type": "message",
        "role": "assistant",
        "model": MODEL,
        "content": content,
        "stop_reason": stop,
        "stop_sequence": null,
        "usage": {"input_tokens": 0, "output_tokens": 0},
    })
}

pub fn skill(mock: &str) -> Result<(String, String), String> {
    let (first, body) = mock.split_once('\n').unwrap_or((mock, ""));
    let name = first.strip_prefix("Launching skill:").map(str::trim).filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).ok_or_else(|| format!("the Skill mock names no skill: {first}"))?;
    let body = body.trim();
    let description = body.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    Ok((format!(".claude/skills/{name}/SKILL.md"), format!("---\nname: {name}\ndescription: {}\n---\n\n{body}\n", json!(description))))
}
