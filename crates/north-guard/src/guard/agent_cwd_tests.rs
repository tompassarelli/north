#![cfg(test)]

use super::agent_cwd::evaluate;
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn spawn(cwd: &str, prompt: &str, kind: &str) -> Verdict {
    let payload = json!({"tool_name": "Agent", "cwd": cwd, "tool_input": {"description": "d", "prompt": prompt, "subagent_type": kind}});
    evaluate(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

#[test]
fn spawn_from_a_lane_cwd_is_denied_unless_the_brief_names_the_lane() {
    let lane = "/home/tom/code/north/worktrees/some-lane";
    let Verdict::Deny(reason) = spawn(lane, "Item: fix a thing.", "worker") else {
        panic!("lane cwd without the lane in the brief must be denied");
    };
    assert!(reason.contains("north/worktrees/some-lane") && reason.contains("cd to the main checkout first"), "{reason}");
    let Verdict::Deny(_) = spawn(&format!("{lane}/crates/x"), "Item: fix.", "") else {
        panic!("a subdirectory of a lane is still the lane");
    };
    assert_eq!(spawn("/home/tom/code/north/main", "Item: fix a thing.", "worker"), Verdict::Allow);
    assert_eq!(spawn("/home/tom/code/north/main/crates", "Item: fix.", "worker"), Verdict::Allow);
    assert_eq!(spawn(lane, &format!("Lane: {lane}. Fix a thing."), "worker"), Verdict::Allow);
    assert_eq!(spawn(lane, "Lane: ~/code/north/worktrees/some-lane. Fix.", "worker"), Verdict::Allow);
    assert_eq!(spawn(lane, "Survey the tests.", "Explore"), Verdict::Allow);
    assert_ne!(spawn("/home/tom/code/north/worktrees/other", "Lane: ~/code/north/worktrees/some-lane.", "worker"), Verdict::Allow);
}
