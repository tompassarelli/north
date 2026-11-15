#![cfg(test)]

use super::{blind_stage, poll_loop, worker_wait};
use crate::hook_input::{HookInput, Verdict};

const COMMAND: &str = "until grep -q '^exit ' /tmp/n719-push.txt; do sleep 3; done; tail -8 /tmp/n719-push.txt | cut -c1-250";
const CALL: &str = "toolu_01BZE2MzCWhRZqNh3uvmTgBK";

fn hook_call(tool: &str, input: serde_json::Value) -> HookInput {
    let data = serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": tool, "agent_type": "worker", "tool_use_id": CALL, "tool_input": input});
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

#[test]
fn a_monitor_call_is_judged_as_a_background_shell_call_so_its_wait_passes_and_its_command_is_still_guarded() {
    let monitor = hook_call("Monitor", serde_json::json!({"command": COMMAND, "description": "safe-push landing result", "timeout_ms": 900000}));
    let foreground = hook_call("Bash", serde_json::json!({"command": COMMAND, "description": "safe-push landing result"}));

    assert_eq!(poll_loop::verdict(&monitor), Verdict::Allow);
    assert_eq!(worker_wait::refusal(&monitor), None);
    let Verdict::Deny(reason) = poll_loop::verdict(&foreground) else { panic!("an inline wait over the limit is refused") };
    let bounded = "timeout 60 bash -c 'until grep -q '\\''^exit '\\'' /tmp/n719-push.txt; do sleep 3; done' && tail -8 /tmp/n719-push.txt | cut -c1-250";
    assert!(reason.contains(&format!("`{bounded}`")), "{reason}");
    assert_eq!(poll_loop::decide_with(bounded, f64::INFINITY, std::path::Path::new("/nonexistent")), None);
    let unsafe_to_rewrite = [
        "until grep -q done /tmp/f; do sleep 3; done",
        "until test -e /tmp/f; do sleep 3; done;",
        "f=/tmp/x; until [ -f $f ]; do sleep 3; done",
        "timeout 5 bash -c x; until test -e /tmp/f; do sleep 3; done",
        "timeout -k 5 600 bash -c 'until test -e /tmp/f; do sleep 3; done'",
    ];
    for command in unsafe_to_rewrite {
        let refused = poll_loop::decide_with(command, f64::INFINITY, std::path::Path::new("/nonexistent")).expect(command);
        assert!(!refused.contains("Run the wait as"), "{command}: {refused}");
    }
    assert!(worker_wait::refusal(&foreground).is_some());
    let staged = hook_call("Monitor", serde_json::json!({"command": "git add -A && until git diff --cached --quiet; do sleep 5; done", "timeout_ms": 900000}));
    assert!(matches!(blind_stage::decide(&staged), Verdict::Deny(_)));
}
