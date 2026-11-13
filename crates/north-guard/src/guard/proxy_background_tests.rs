#![cfg(test)]

use super::proxy_role::{self, ClaudeStart, Registered, World};
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

    fn registered_proxy(&self) -> Option<Registered> {
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

    fn break_glass(&self) -> bool {
        false
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
fn the_proxy_backgrounds_a_call_that_waits() {
    let proxy = Role("proxy");
    for (command, class) in [
        ("north fleet launch --task north#1048", "a fleet launch"),
        ("north fleet revive proxy", "a fleet launch"),
        ("north fleet dispatch", "a fleet launch"),
        ("ssh nexus 'north fleet launch --task north#1048'", "a fleet launch"),
        ("north job-run nightly -- north fleet tick", "a job"),
    ] {
        let Verdict::Deny(reason) = proxy_role::decide(&proxy, &bash(command, false)) else { panic!("{command} held the proxy's turn open in the foreground") };
        assert!(reason.contains(class), "{command}: {reason}");
        assert!(reason.contains("run_in_background: true") && reason.contains("north fleet launch"), "{command}: {reason}");
        assert_eq!(proxy_role::decide(&proxy, &bash(command, true)), Verdict::Allow, "{command} in the background");
    }
    for open in [
        "north fleet tree",
        "north fleet launch --help",
        "gh issue view 1048",
        "timeout 30 north fleet launch --task north#1048",
        "timeout 20s ssh nexus north fleet tree",
        "north job-run --notify nightly -- north fleet tick",
        "ssh nexus north fleet tree",
        "ssh -T nexus -- 'north land status | head -20'",
        "bash -c 'north fleet tree'",
    ] {
        assert_eq!(proxy_role::decide(&proxy, &bash(open, false)), Verdict::Allow, "{open}");
    }
    let Verdict::Deny(reason) = proxy_role::decide(&proxy, &bash("timeout 120 north fleet launch --task north#1048", false)) else { panic!("a 120 s bound still lets a launch wait past 30 s") };
    assert!(reason.contains("a fleet launch"), "{reason}");
    let Verdict::Deny(reason) = proxy_role::decide(&proxy, &bash("bash -c 'north fleet launch --task north#1048'", false)) else { panic!("a shell wrapper hid the launch") };
    assert!(reason.contains("a fleet launch"), "{reason}");
    assert_eq!(proxy_role::decide(&proxy, &bounded_bash("north fleet launch --task north#1048", 30_000)), Verdict::Allow, "the tool's own 30 s bound");
    assert_eq!(proxy_role::decide(&Role("worker"), &bash("north fleet launch --task north#1048", false)), Verdict::Allow, "a worker is not the proxy");
}
