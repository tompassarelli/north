use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Guard {
    unit: &'static str,
    agent_type: Option<&'static str>,
    command: &'static str,
}

const GUARDS: &[Guard] = &[
    Guard {
        unit: "session-kill-guard",
        agent_type: None,
        command: "kill -1",
    },
    Guard {
        unit: "clipboard-guard",
        agent_type: None,
        command: "CHECK=1 wl-copy hi",
    },
    Guard {
        unit: "worker-wait-guard",
        agent_type: Some("worker"),
        command: "cd x && safe-push --to main",
    },
    Guard {
        unit: "native-launch-guard",
        agent_type: None,
        command: "env FOO=1 wine game.exe",
    },
    Guard {
        unit: "native-item-guard",
        agent_type: None,
        command: "gh issue comment 5 --body life#1",
    },
];

fn decision(guard: &Guard, activation: &Path, disabled: &str) -> Option<String> {
    let mut payload = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": "/tmp",
        "tool_input": {"command": guard.command},
    });
    if let Some(agent_type) = guard.agent_type {
        payload["agent_type"] = json!(agent_type);
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", "pre-bash"])
        .env_remove("NORTH_HOOK_ID")
        .env("NORTH_AGENT_ACTIVE", activation)
        .env("AGENT_NO_AUTHORING_HOOKS", disabled)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).ok()?;
    response["hookSpecificOutput"]["permissionDecision"]
        .as_str()
        .map(str::to_owned)
}

fn each_command_guard_denies_only_while_its_hook_is_active() {
    let scratch = tempfile::tempdir().unwrap();
    for guard in GUARDS {
        let active = scratch.path().join(format!("{}.active", guard.unit));
        std::fs::write(&active, format!("hook {}\n", guard.unit)).unwrap();
        let other = scratch.path().join(format!("{}.other", guard.unit));
        std::fs::write(&other, "hook unrelated-guard\n").unwrap();
        let missing = scratch.path().join("missing.active");
        for (activation, disabled, expected) in [
            (&active, "", Some("deny")),
            (&active, "0", Some("deny")),
            (&active, "1", None),
            (&other, "", None),
            (&missing, "", None),
        ] {
            assert_eq!(
                decision(guard, activation, disabled).as_deref(),
                expected,
                "{} with {} disabled={disabled:?}",
                guard.unit,
                activation.display()
            );
        }
    }
}

fn malformed_payloads_pass_silently() {
    let scratch = tempfile::tempdir().unwrap();
    let active = scratch.path().join("all.active");
    std::fs::write(&active, GUARDS.iter().map(|g| format!("hook {}\n", g.unit)).collect::<String>()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", "pre-bash"])
        .env("NORTH_AGENT_ACTIVE", &active)
        .env("AGENT_NO_AUTHORING_HOOKS", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(br#"{"tool_name":"#).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn command_guards_follow_their_hook_activation() {
    each_command_guard_denies_only_while_its_hook_is_active();
    malformed_payloads_pass_silently();
}
