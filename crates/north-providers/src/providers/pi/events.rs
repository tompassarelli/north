use serde_json::Value;

use crate::fleet::model::EntryKind;

use super::super::opencode::harness::{Tally, tool_text};

fn is_assistant(event: &Value) -> bool {
    event["message"]["role"] == "assistant"
}

/// Reads one `--mode json` event of Pi: an assistant message that ends carries the turn's usage and, when the provider refused it, the error; a tool that starts is work and a tool call, and a compaction that ends with a result is a compaction.
pub fn read(tally: &mut Tally, event: &Value) {
    match event["type"].as_str() {
        Some("message_end") if is_assistant(event) => {
            let usage = &event["message"]["usage"];
            let count = |key: &str| usage[key].as_u64().unwrap_or(0);
            tally.spend(count("input"), count("output"), count("cacheRead"), count("cacheWrite"));
            if event["message"]["stopReason"] == "error" {
                tally.error = Some(event["message"]["errorMessage"].as_str().unwrap_or("error").to_owned());
            } else {
                tally.error = None;
                tally.progressed = true;
            }
        }
        Some("tool_execution_start") => {
            tally.progressed = true;
            tally.tool_calls += 1;
        }
        Some("compaction_end") if event["result"].is_object() => tally.compactions += 1,
        _ => {}
    }
}

pub fn entries(event: &Value) -> Vec<(EntryKind, String)> {
    match event["type"].as_str() {
        Some("message_end") if is_assistant(event) => {
            let text: String = event["message"]["content"].as_array().into_iter().flatten().filter(|block| block["type"] == "text").filter_map(|block| block["text"].as_str()).collect();
            let error = (event["message"]["stopReason"] == "error").then(|| (EntryKind::Error, event["message"]["errorMessage"].as_str().unwrap_or("error").to_owned()));
            [Some((EntryKind::Assistant, text)), error].into_iter().flatten().collect()
        }
        Some("tool_execution_start") => vec![(EntryKind::Tool, tool_text(event["toolName"].as_str().unwrap_or("tool"), &event["args"]))],
        _ => Vec::new(),
    }
}
