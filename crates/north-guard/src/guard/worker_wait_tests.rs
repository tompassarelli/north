#![cfg(test)]

use super::worker_wait::refusal;
use crate::hook::HookInput;
use serde_json::json;

#[test]
fn a_worker_monitor_call_is_not_a_foreground_wait() {
    let payload = json!({
        "tool_name": "Bash",
        "agent_type": "worker",
        "tool_input": {"command": "until grep -q done f; do sleep 20; done", "description": "f done", "timeout_ms": 600000},
    });
    assert!(refusal(&HookInput::parse(payload.to_string().as_bytes()).unwrap()).is_none());
}

