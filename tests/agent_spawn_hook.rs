use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn run(activation: &Path, home: &Path, disabled: &str, event: &Value) -> Option<String> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", "pre-agent"])
        .env_remove("NORTH_HOOK_ID")
        .env("HOME", home)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("SPAWN_CAPACITY_HOME", home)
        .env("NORTH_AGENT_ACTIVE", activation)
        .env("AGENT_NO_AUTHORING_HOOKS", disabled)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(event.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).ok()?;
    response["hookSpecificOutput"]["permissionDecision"].as_str().map(str::to_owned)
}

fn a_tier_refusal_releases_the_pending_grant_use_and_a_disabled_guard_keeps_it() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    let state = home.join(".local/state/agents");
    std::fs::create_dir_all(&state).unwrap();
    let activation = scratch.path().join("activation.active");
    std::fs::write(&activation, "hook worker-tier-guard\n").unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let grant = json!({"account": "claude:a", "session": "parent", "uses": 1, "pending": [{"id": "denied", "at": now}]});
    let event = json!({
        "hook_event_name": "PreToolUse", "tool_name": "Agent", "session_id": "parent", "tool_use_id": "denied",
        "tool_input": {"subagent_type": "worker", "prompt": "Category: feature"},
    });
    for (disabled, expected, pending) in [("1", None, 1), ("0", Some("deny"), 0)] {
        std::fs::write(state.join("pace-control.json"), json!({"grants": [grant]}).to_string()).unwrap();
        assert_eq!(run(&activation, &home, disabled, &event).as_deref(), expected);
        let saved: Value = serde_json::from_slice(&std::fs::read(state.join("pace-control.json")).unwrap()).unwrap();
        assert_eq!(saved["grants"][0]["uses"], 1, "{saved}");
        assert_eq!(saved["grants"][0]["pending"].as_array().unwrap().len(), pending, "{saved}");
    }
}

fn the_tier_guard_denies_only_while_its_hook_is_active() {
    let scratch = tempfile::tempdir().unwrap();
    let event = json!({
        "hook_event_name": "PreToolUse", "tool_name": "Agent",
        "tool_input": {"subagent_type": "worker-high", "prompt": "Category: feature\nSpec: measured\nScope: module\nSurface: shell\nVerify: local-test"},
    });
    let active = scratch.path().join("on.active");
    std::fs::write(&active, "hook worker-tier-guard\n").unwrap();
    let other = scratch.path().join("other.active");
    std::fs::write(&other, "hook adjacent-hook\n").unwrap();
    for (activation, disabled, expected) in [
        (&active, "", Some("deny")),
        (&active, "1", None),
        (&other, "", None),
        (&scratch.path().join("missing.active"), "", None),
    ] {
        assert_eq!(run(activation, scratch.path(), disabled, &event).as_deref(), expected, "{} {disabled:?}", activation.display());
    }
}

#[test]
fn the_tier_guard_follows_its_hook_activation_and_releases_a_refused_grant_use() {
    a_tier_refusal_releases_the_pending_grant_use_and_a_disabled_guard_keeps_it();
    the_tier_guard_denies_only_while_its_hook_is_active();
}
