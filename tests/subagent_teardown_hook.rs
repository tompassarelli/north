use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn run(payload: &Value, activation: &Path, disabled: &str) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", "subagent-stop"])
        .env_remove("NORTH_HOOK_ID")
        .env("NORTH_AGENT_ACTIVE", activation)
        .env("AGENT_NO_AUTHORING_HOOKS", disabled)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn subagent_teardown_follows_its_hook_activation() {
    let scratch = tempfile::tempdir().unwrap();
    std::fs::create_dir(scratch.path().join("subagents")).unwrap();
    let transcript = scratch.path().join("subagents/agent-a1.jsonl");
    let lines = [
        json!({"type":"assistant","timestamp":"2099-01-01T00:00:00.000Z","message":{"content":[{"type":"tool_use","id":"toolu_mon","name":"Monitor","input":{"description":"CI watch","timeout_ms":600000}}]}}),
        json!({"type":"user","timestamp":"2099-01-01T00:00:00.000Z","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_mon","content":"Monitor started (task bmon0001)."}]},"toolUseResult":{"taskId":"bmon0001"}}),
    ];
    std::fs::write(&transcript, lines.map(|line| format!("{line}\n")).concat()).unwrap();
    let active = scratch.path().join("on.active");
    std::fs::write(&active, "hook north-hook\n").unwrap();
    let other = scratch.path().join("other.active");
    std::fs::write(&other, "hook unrelated-guard\n").unwrap();
    let missing = scratch.path().join("missing.active");
    let payload = |active: bool| {
        json!({"hook_event_name":"SubagentStop","stop_hook_active":active,"transcript_path":scratch.path().join("main.jsonl"),"agent_transcript_path":transcript})
    };
    let blocked = "{\"decision\": \"block\", \"reason\": \"You still have background tasks running: bmon0001 (CI watch). Stop each with TaskStop (long work that must outlive you runs as `job-run --notify`), then finish your report.\"}\n";
    for (activation, disabled, active_flag, expected) in [
        (&active, "", false, blocked),
        (&active, "0", false, blocked),
        (&active, "1", false, ""),
        (&active, "", true, ""),
        (&other, "", false, ""),
        (&missing, "", false, ""),
    ] {
        assert_eq!(
            run(&payload(active_flag), activation, disabled),
            expected,
            "{} disabled={disabled:?} stop_hook_active={active_flag}",
            activation.display()
        );
    }
}
