use super::worker_wait::refusal;
use crate::hook::HookInput;
use serde_json::{Value, json};

const CAPACITY: &str = "bun /opt/skills/machine-capacity/scripts/machine-capacity.mjs";

fn refused(agent: Option<&str>, background: bool, timeout: Option<u64>, command: &str) -> bool {
    let mut tool_input = json!({"command": command});
    if background {
        tool_input["run_in_background"] = Value::Bool(true);
    }
    if let Some(timeout) = timeout {
        tool_input["timeout"] = json!(timeout);
    }
    let mut payload = json!({"tool_name": "Bash", "tool_input": tool_input});
    if let Some(agent) = agent {
        payload["agent_type"] = json!(agent);
    }
    let input = HookInput::parse(payload.to_string().as_bytes()).unwrap();
    refusal(&input).is_some()
}

fn worker(command: &str) -> bool {
    refused(Some("worker"), false, None, command)
}

fn foreground_waits_that_can_pass_a_minute_are_refused_for_workers() {
    for command in [
        "until grep -q done f; do sleep 20; done",
        "while true; do sleep 5; done",
        "timeout 900 safe-push --to main",
        "safe-push --to main",
        "cd x && safe-push --to main",
        "FOO=1 safe-push",
        "/home/tom/.local/bin/safe-push --to main",
        "bun run test",
        "bun test",
        "sleep 120",
        "timeout 300 cargo build",
        "gh run watch 123",
        "gh pr checks 5 --watch",
        "bun wisp farm run",
        &format!("{CAPACITY} run --owner o --timeout-seconds 600 -- bun wisp view s o"),
        &format!("{CAPACITY} session --class native -- wine game.exe"),
    ] {
        assert!(worker(command), "{command}");
    }
}

fn short_commands_and_inert_mentions_pass() {
    let heredoc = "git commit -F - <<'EOF'\nsafe-push lands it\nEOF";
    for command in [
        "git status",
        "bun test test/a.test.ts",
        "sleep 5",
        "which safe-push",
        "wc -l $(command -v safe-push)",
        "git commit -m \"land through safe-push\"",
        "git commit -m 'gh run watch 123 in prose'",
        heredoc,
    ] {
        assert!(!worker(command), "{command}");
    }
}

fn a_declared_timeout_over_a_minute_is_refused() {
    assert!(refused(Some("worker"), false, Some(300_000), "bun run check"));
    assert!(!refused(Some("worker"), false, Some(30_000), "git status"));
    assert!(!refused(Some("worker"), false, Some(60_000), "git status"));
}

fn only_foreground_worker_calls_are_bound() {
    let wait = "until grep -q done f; do sleep 20; done";
    assert!(!refused(Some("worker"), true, Some(600_000), wait));
    assert!(!refused(Some("worker"), true, None, "safe-push --to main"));
    assert!(!refused(None, false, None, wait));
    assert!(!refused(Some("Explore"), false, None, "safe-push --to main"));
    assert!(refused(Some("worker-high"), false, None, "safe-push --to main"));
    assert!(refused(Some("worker-sonnet"), false, None, "sleep 90"));
}

fn recorded_decisions_of_the_retired_shell_guard_still_hold() {
    let cases: Vec<(String, String)> =
        serde_json::from_str(include_str!("fixtures/worker_wait.json")).unwrap();
    for (command, expected) in cases {
        let observed = if worker(&command) { "deny" } else { "allow" };
        assert_eq!(observed, expected, "{command}");
    }
}

#[test]
fn worker_wait_decisions_hold() {
    foreground_waits_that_can_pass_a_minute_are_refused_for_workers();
    short_commands_and_inert_mentions_pass();
    a_declared_timeout_over_a_minute_is_refused();
    only_foreground_worker_calls_are_bound();
    recorded_decisions_of_the_retired_shell_guard_still_hold();
}
