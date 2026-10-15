use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};

fn bash(command: &str) -> HookInput {
    let payload = json!({"tool_name":"Bash", "cwd":"/home/test/code/proj/main", "tool_input":{"command":command}});
    HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
}

fn replay(fixture: &str, decide: fn(&HookInput) -> Verdict) {
    let fixture: Value = serde_json::from_str(fixture).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(!cases.is_empty());
    for case in cases {
        let command = case[0].as_str().unwrap();
        let expected = match fixture["reasons"][case[1].as_u64().unwrap() as usize].as_str() {
            Some(reason) => Verdict::Deny(reason.into()),
            None => Verdict::Allow,
        };
        assert_eq!(decide(&bash(command)), expected, "{command:?}");
    }
}

fn envelopes_without_literal_git_or_object_input_allow(
    command: &str,
    decide: fn(&HookInput) -> Verdict,
) {
    assert!(matches!(decide(&bash(command)), Verdict::Deny(_)));
    let payload = json!({"tool_name":"Bash", "tool_input":{"command":command}});
    let escaped = payload.to_string().replace("git", "\\u0067it");
    assert_eq!(
        decide(&HookInput::parse(escaped.as_bytes()).unwrap()),
        Verdict::Allow
    );
    let string_input = json!({"tool_name":"Bash", "tool_input":payload["tool_input"].to_string()});
    assert_eq!(
        decide(&HookInput::parse(string_input.to_string().as_bytes()).unwrap()),
        Verdict::Allow
    );
    let codex = json!({"tool_name":"exec_command", "tool_input":{"cmd":command}});
    assert_eq!(
        decide(&HookInput::parse(codex.to_string().as_bytes()).unwrap()),
        Verdict::Allow
    );
}

#[test]
fn git_stash_guard_replays_original_decisions() {
    replay(include_str!("fixtures/git_stash.json"), super::git_stash::decide);
    envelopes_without_literal_git_or_object_input_allow(
        "git stash pop",
        super::git_stash::decide,
    );
}

#[test]
fn blind_stage_guard_replays_original_decisions() {
    replay(include_str!("fixtures/blind_stage.json"), super::blind_stage::decide);
    envelopes_without_literal_git_or_object_input_allow("git add -A", super::blind_stage::decide);
}
