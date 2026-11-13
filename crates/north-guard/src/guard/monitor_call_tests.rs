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

#[test]
fn worker_wait_refuses_a_real_foreground_wait_and_not_its_text_in_heredocs_quotes_paths_or_short_bounds() {
    let refused = [
        "cd /home/tom/code/wisp/worktrees/simple-texcoord && safe-push --to main 2>&1 | tail -20",
        "until rg -q done /tmp/x; do sleep 5; done",
        "cd ~/code/makgora/worktrees/wc3-1075/ts && timeout 300 bun install --frozen-lockfile 2>&1 | tail -2",
        "cd /home/tom/code/north/worktrees/n281-names && bun test harness/code-health 2>&1 | tail -4",
        "timeout 90 bash -c 'until test -e /tmp/x; do sleep 1; done'",
    ];
    let allowed = [
        "cat > /tmp/add393.md <<'EOF'\nuntil pgrep -f x; do sleep 1; done\nEOF",
        "cd /tmp/goldrev && B=/home/tom/code/north/worktrees/north-621-p8/bin/safe-push && sed -n '22,26p;30,36p' $B > fns.sh",
        "cd ~/code/north/worktrees/land-runner-auto && f=bin/safe-push.test.sh && perl -0pi -e 's/a/b/' $f",
        "cd /home/tom/code/north/worktrees/runtime-no-gex45-pull && north review-gate id origin/main HEAD $(safe-push --durable-harness-paths north) 2>&1 | tail -5",
        "which safe-push north; safe-push --help 2>&1 | head -40",
        "timeout 55 bash -c 'while ! grep -q \"^exit=\" /tmp/x.output; do sleep 3; done'; cat /tmp/x.output",
        "tail -c 600 /tmp/obs-bun.log; ps -eo pid,etime,args | rg 'bun test|tsc' | rg -v rg | head",
    ];
    for command in refused {
        assert!(worker_wait::blocking(command, None).is_some(), "{command}");
    }
    for command in allowed {
        assert_eq!(worker_wait::blocking(command, None), None, "{command}");
    }
    assert!(worker_wait::blocking("chk.sh check -p north-core", Some(400.0)).is_some());
}
