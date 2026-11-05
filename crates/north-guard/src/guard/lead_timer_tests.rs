#![cfg(test)]

use super::lead_timer::refusal;
use crate::hook::HookInput;
use serde_json::json;
use std::fs;
use std::path::Path;

fn put(root: &Path, directory: &str, name: &str, parent: Option<&str>) {
    let dir = root.join(directory);
    fs::create_dir_all(&dir).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/", "model": null, "created_ms": 1, "host_pid": null, "conversation": null, "parent": parent});
    fs::write(dir.join("session.json"), record.to_string()).unwrap();
}

fn fleet() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    put(root.path(), "north-burndown", "north-burndown", Some("proxy"));
    put(root.path(), "box-worker", "box-worker", Some("north-burndown"));
    put(root.path(), ".archive/old-worker", "old-worker", Some("archived-parent"));
    root
}

fn call(tool: &str, subagent: bool) -> HookInput {
    let mut payload = json!({"hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": {"prompt": "check the fleet", "delaySeconds": 600}});
    if subagent {
        payload["agent_type"] = json!("worker");
    }
    HookInput::parse(payload.to_string().as_bytes()).unwrap()
}

#[test]
fn timer_calls_are_refused_in_a_fleet_lead() {
    let root = fleet();
    for session in ["proxy", "north-burndown", "archived-parent", "wisp-lead", "triage-lead-3"] {
        for tool in ["ScheduleWakeup", "CronCreate"] {
            let reason = refusal(&call(tool, false), Some(session), root.path()).unwrap_or_else(|| panic!("{session} {tool} allowed"));
            assert!(reason.contains(tool) && reason.contains("End your turn"), "{reason}");
            assert!(reason.contains("child reports") && reason.contains("waits") && reason.contains("flow` alerts"), "{reason}");
        }
    }
}

#[test]
fn timer_calls_pass_outside_a_fleet_lead() {
    let root = fleet();
    for tool in ["ScheduleWakeup", "CronCreate"] {
        assert_eq!(refusal(&call(tool, false), None, root.path()), None, "an interactive session Tom drives");
        assert_eq!(refusal(&call(tool, false), Some(""), root.path()), None);
        assert_eq!(refusal(&call(tool, false), Some("box-worker"), root.path()), None, "a box worker with no children");
        assert_eq!(refusal(&call(tool, true), Some("north-burndown"), root.path()), None, "a lead's subagent");
    }
    assert_eq!(refusal(&call("Bash", false), Some("north-burndown"), root.path()), None);
    assert_eq!(refusal(&call("Agent", false), Some("proxy"), root.path()), None);
}

#[test]
fn the_recorded_role_decides_whether_a_session_is_a_lead() {
    let root = tempfile::tempdir().unwrap();
    let put_role = |name: &str, role: &str, parent: Option<&str>| {
        let dir = root.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/", "model": null, "created_ms": 1, "host_pid": null, "parent": parent, "role": role});
        fs::write(dir.join("session.json"), record.to_string()).unwrap();
    };
    put_role("proxy", "proxy", None);
    put_role("fleet-staffing", "lead", None);
    put_role("lead-fixer", "worker", Some("fleet-staffing"));
    put_role("sub-worker", "worker", Some("lead-fixer"));
    for session in ["proxy", "fleet-staffing"] {
        assert!(refusal(&call("ScheduleWakeup", false), Some(session), root.path()).is_some(), "{session} allowed");
    }
    assert_eq!(refusal(&call("ScheduleWakeup", false), Some("lead-fixer"), root.path()), None, "a worker with lead in its name and a child of its own is no lead");
}
