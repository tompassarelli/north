#![cfg(test)]

use super::worktree_add::decide;
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn run(command: &str, cwd: &str) -> Verdict {
    decide(&HookInput::parse(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": command}, "cwd": cwd}).to_string().as_bytes()).unwrap())
}

#[test]
fn a_lane_path_relative_to_the_main_checkout_is_refused_as_nested_in_main() {
    let nesting = "cd /home/tom/code && for r in north south; do git -C $r/main fetch -q origin main && git -C $r/main worktree add -q -b fix/config-drift-993 $r/worktrees/config-drift-993 origin/main && echo ok $r; done";
    let from_main = "git worktree add -q --detach north/worktrees/config-drift-993 origin/main";
    let with_dash_c = "git -C north/main worktree add -b fix/x north/worktrees/x origin/main";
    for (command, cwd) in [(nesting, "/home/tom/code"), (from_main, "/home/tom/code/north/main"), (with_dash_c, "/home/tom/code")] {
        assert!(matches!(run(command, cwd), Verdict::Deny(reason) if reason.contains("north lane new")), "{command}");
    }
}

#[test]
fn lanes_that_resolve_to_the_project_container_are_allowed() {
    let right = [
        ("git -C /home/tom/code/north/main worktree add -b x /home/tom/code/north/worktrees/x origin/main", "/home/tom"),
        ("git -C main worktree add ../worktrees/x origin/main", "/home/tom/code/north"),
        ("git -C ~/code/north/main worktree add -b x ~/code/north/worktrees/x", "/home/tom"),
        ("git worktree add ../worktrees/x", "/home/tom/code/north/main"),
        ("git -C /home/tom/code/main/main worktree add -b y /home/tom/code/main/worktrees/y", "/"),
    ];
    for (command, cwd) in right {
        assert!(matches!(run(command, cwd), Verdict::Allow), "{command}");
    }
}
