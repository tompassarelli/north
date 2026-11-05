#![cfg(test)]

use super::subagent_message::notice;
use crate::hook::HookInput;
use serde_json::json;
use std::fs;
use std::path::Path;

fn put(root: &Path, directory: &str, name: &str) {
    let dir = root.join(directory);
    fs::create_dir_all(&dir).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/", "model": null, "created_ms": 1, "host_pid": null, "conversation": null, "parent": "proxy"});
    fs::write(dir.join("session.json"), record.to_string()).unwrap();
}

fn fleet() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "north-burndown", "north-burndown");
    put(root.path(), ".archive/old-worker", "old-worker");
    root
}

fn send(tool: &str, to: &str) -> HookInput {
    let payload = json!({"hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": {"to": to, "message": "stop and report"}});
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

#[test]
fn a_message_to_an_agent_subagent_tells_the_sender_the_next_tool_round_delay() {
    let root = fleet();
    for (to, name) in [("a1b2c3", "a1b2c3"), ("reviewer", "reviewer"), ("old-worker", "old-worker"), ("reviewer [r7]", "reviewer")] {
        for root in [Some(root.path()), None] {
            let text = notice(&send("SendMessage", to), root).unwrap_or_else(|| panic!("{to} got no notice"));
            assert!(text.starts_with(&format!("subagent-message-delay: {name} ")), "{text}");
            assert!(text.contains("next tool round") && text.contains("when that call returns"), "{text}");
        }
    }
}

#[test]
fn a_message_to_a_live_fleet_session_passes_silently() {
    let root = fleet();
    assert_eq!(notice(&send("SendMessage", "north-burndown"), Some(root.path())), None);
    assert_eq!(notice(&send("SendMessage", "north-burndown [b2]"), Some(root.path())), None);
    assert_eq!(notice(&send("SendMessage", ""), Some(root.path())), None);
    assert_eq!(notice(&send("ListAgents", "reviewer"), Some(root.path())), None);
}
