use crate::hook::{HookInput, Verdict};
use serde_json::{Value, json};

fn call(tool: &str, input: Value) -> HookInput {
    let payload = json!({"tool_name":tool, "cwd":std::env::temp_dir(), "tool_input":input});
    HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
}

fn bash(command: &str) -> HookInput {
    call("Bash", json!({"command":command}))
}

fn shell_calls(command: &str) -> Vec<HookInput> {
    vec![
        bash(command),
        call("exec_command", json!({"cmd":command})),
        call("functions.exec_command", json!({"cmd":command, "workdir":"/tmp"})),
        call("shell", json!({"command":["bash", "-lc", command]})),
        call("shell_command", json!({"command":command})),
    ]
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
        for call in shell_calls(command) {
            assert_eq!(decide(&call), expected, "{} {command:?}", call.tool_name());
        }
    }
}

fn envelopes_without_literal_git_allow_and_string_input_denies(
    command: &str,
    decide: fn(&HookInput) -> Verdict,
) {
    for call in shell_calls(command) {
        assert!(matches!(decide(&call), Verdict::Deny(_)), "{}", call.tool_name());
    }
    let payload = json!({"tool_name":"Bash", "tool_input":{"command":command}});
    let escaped = payload.to_string().replace("git", "\\u0067it");
    assert_eq!(
        decide(&HookInput::parse(escaped.as_bytes()).unwrap()),
        Verdict::Allow
    );
    let string_input = json!({"tool_name":"exec_command", "tool_input":json!({"cmd":command}).to_string()});
    assert!(matches!(
        decide(&HookInput::parse(string_input.to_string().as_bytes()).unwrap()),
        Verdict::Deny(_)
    ));
    let codex = json!({"tool_name":"exec_command", "tool_input":{"cmd":command}}).to_string();
    let escaped = codex.replace("git", "\\u0067it");
    assert_eq!(
        decide(&HookInput::parse(escaped.as_bytes()).unwrap()),
        Verdict::Allow
    );
    let edit = json!({"tool_name":"Edit", "tool_input":{"cmd":command, "command":command}});
    assert_eq!(
        decide(&HookInput::parse(edit.to_string().as_bytes()).unwrap()),
        Verdict::Allow
    );
}

#[test]
fn git_stash_guard_replays_original_decisions() {
    replay(include_str!("fixtures/git_stash.json"), super::git_stash::decide);
    envelopes_without_literal_git_allow_and_string_input_denies(
        "git stash pop",
        super::git_stash::decide,
    );
}

#[test]
fn blind_stage_guard_replays_original_decisions() {
    replay(include_str!("fixtures/blind_stage.json"), super::blind_stage::decide);
    envelopes_without_literal_git_allow_and_string_input_denies("git add -A", super::blind_stage::decide);
}
