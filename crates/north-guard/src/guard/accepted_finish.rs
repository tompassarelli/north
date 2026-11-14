use serde_json::Value;

use crate::hook_input::{HookInput, Verdict};

const ENDED: &str = "the finish call queued the lane, so the turn ends here; a lander refusal starts the next one";
const ACCEPTED: [&str; 2] = ["Done: queued", "finish: ✓"];

/// Ends a session's turn at its accepted `finish` call, so the client asks the model nothing after the ticket.
/// Claude Code fires PostToolUse for a finish call the tool server answered as an error too, so only a result naming a ticket or a recorded draft ends the turn, and a refusal reaches the model (north#1509).
pub fn evaluate(input: &HookInput) -> Verdict {
    let finish = input.data["hook_event_name"] == "PostToolUse" && input.tool_name() == crate::fleet::tickets::FINISH_TOOL;
    if finish && accepted(&input.data["tool_response"]) { Verdict::End(ENDED.to_owned()) } else { Verdict::Allow }
}

fn accepted(response: &Value) -> bool {
    text(response).lines().any(|line| ACCEPTED.iter().any(|mark| line.starts_with(mark)))
}

fn text(response: &Value) -> String {
    match response {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().map(text).collect::<Vec<_>>().join("\n"),
        Value::Object(fields) => ["text", "content"].iter().filter_map(|key| fields.get(*key)).map(text).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}
