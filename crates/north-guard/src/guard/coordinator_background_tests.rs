#![cfg(test)]

use super::coordinator_role::{self, ClaudeStart, Registered, World};
use crate::hook_input::{HookInput, Verdict};
use serde_json::json;

struct Role(&'static str);

impl World for Role {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "HOME" => Some("/home/tom".into()),
            "NORTH_FLEET_ROLE" => Some(self.0.into()),
            "NORTH_FLEET_SESSION" => Some(self.0.into()),
            _ => None,
        }
    }

    fn registered_coordinator(&self) -> Option<Registered> {
        None
    }

    fn profile(&self, _account: &str) -> Option<String> {
        None
    }

    fn claude_start(&self) -> Option<ClaudeStart> {
        None
    }

    fn lane_files(&self, _cwd: &str) -> Option<Vec<String>> {
        None
    }
    fn lane_file_bytes(&self, _cwd: &str, _file: &str) -> Option<Vec<u8>> {
        None
    }

    fn status_verb(&self, words: &[&str]) -> bool {
        matches!(words, ["land", "status", ..] | ["glance", ..])
    }
}

fn bash(command: &str, background: bool) -> HookInput {
    let data = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "s1",
        "cwd": "/home/tom/code/north/main",
        "tool_name": "Bash",
        "tool_input": {"command": command, "run_in_background": background},
    });
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

fn bounded_bash(command: &str, timeout_ms: u64) -> HookInput {
    let data = json!({
        "hook_event_name": "PreToolUse",
        "session_id": "s1",
        "cwd": "/home/tom/code/north/main",
        "tool_name": "Bash",
        "tool_input": {"command": command, "timeout": timeout_ms},
    });
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

#[test]
fn the_coordinator_backgrounds_a_call_that_waits() {
    let coordinator = Role("coordinator");
    for (command, class) in [
        ("north cloud launch --task north#1048", "a cloud launch"),
        ("scp /tmp/a nexus:/tmp/b", "a remote copy"),
        ("ssh nexus 'north runtime status'", "a remote command"),
        ("north job-run nightly -- true", "a job"),
    ] {
        let Verdict::Deny(reason) = coordinator_role::decide(&coordinator, &bash(command, false)) else { panic!("{command} held the coordinator's turn open in the foreground") };
        assert!(reason.contains(class), "{command}: {reason}");
        assert!(reason.contains("run_in_background: true") && !reason.contains("north fleet launch"), "{command}: {reason}");
        assert_eq!(coordinator_role::decide(&coordinator, &bash(command, true)), Verdict::Allow, "{command} in the background");
    }
    for open in [
        "north cloud launch --help",
        "gh issue view 1048",
        "timeout 30 north cloud launch --task north#1048",
        "timeout 20s ssh nexus north runtime status",
        "north job-run --notify nightly -- true",
        "ssh -T nexus -- 'north land status | head -20'",
        "bash -c 'gh issue view 1048'",
    ] {
        assert_eq!(coordinator_role::decide(&coordinator, &bash(open, false)), Verdict::Allow, "{open}");
    }
    let Verdict::Deny(reason) = coordinator_role::decide(&coordinator, &bash("timeout 120 north cloud launch --task north#1048", false)) else { panic!("a 120 s bound still lets a launch wait past 30 s") };
    assert!(reason.contains("a cloud launch"), "{reason}");
    let Verdict::Deny(reason) = coordinator_role::decide(&coordinator, &bash("bash -c 'north cloud launch --task north#1048'", false)) else { panic!("a shell wrapper hid the launch") };
    assert!(reason.contains("a cloud launch"), "{reason}");
    assert_eq!(coordinator_role::decide(&coordinator, &bounded_bash("north cloud launch --task north#1048", 30_000)), Verdict::Allow, "the tool's own 30 s bound");
    assert_eq!(coordinator_role::decide(&Role("worker"), &bash("north cloud launch --task north#1048", false)), Verdict::Allow, "a worker is not the coordinator");
}
