#![cfg(test)]

use super::{poll_loop, worker_wait};
use crate::hook::{HookInput, Verdict};

const COMMAND: &str = "until grep -q '^exit ' /tmp/n719-push.txt; do sleep 3; done; tail -8 /tmp/n719-push.txt | cut -c1-250";
const CALL: &str = "toolu_01BZE2MzCWhRZqNh3uvmTgBK";

fn hook_call(transcript: Option<&std::path::Path>) -> HookInput {
    let mut data = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "agent_type": "worker",
        "tool_use_id": CALL,
        "tool_input": {"command": COMMAND, "description": "safe-push landing result"},
    });
    if let Some(path) = transcript {
        data["transcript_path"] = path.to_string_lossy().into_owned().into();
    }
    HookInput::parse(data.to_string().as_bytes()).unwrap()
}

#[test]
fn a_monitor_until_loop_handed_to_the_bash_hooks_without_timeout_ms_is_allowed_once_the_transcript_names_it_monitor() {
    let directory = tempfile::tempdir().unwrap();
    let transcript = directory.path().join("7d250abd-5894-4e53-9594-cd0b79c7d9b9.jsonl");
    let call = serde_json::json!({"type": "assistant", "message": {"role": "assistant", "content": [
        {"type": "tool_use", "id": CALL, "name": "Monitor", "input": {"command": COMMAND, "description": "safe-push landing result", "timeout_ms": 900000}}
    ]}});
    std::fs::write(&transcript, format!("{call}\n")).unwrap();

    assert!(matches!(poll_loop::verdict(&hook_call(None)), Verdict::Deny(_)));
    assert!(worker_wait::refusal(&hook_call(None)).is_some());
    assert_eq!(poll_loop::verdict(&hook_call(Some(&transcript))), Verdict::Allow);
    assert_eq!(worker_wait::refusal(&hook_call(Some(&transcript))), None);
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
