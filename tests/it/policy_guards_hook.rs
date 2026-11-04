use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const POLICY: &str = "Never hand-edit generated agent policy";
const MODEL: &str = "a todo model field must name an exact concrete runtime model";

fn run(kind: &str, home: &Path, payload: &Value, off: bool) -> String {
    let active = home.join("activation.active");
    std::fs::write(&active, "hook north-hook\n").unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", kind])
        .env_remove("NORTH_HOOK_ID")
        .env("HOME", home)
        .env("TODO_ROOT", home.join("code/todo"))
        .env("NORTH_AGENT_ACTIVE", &active)
        .env("AGENT_NO_AUTHORING_HOOKS", if off { "1" } else { "0" })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn generated_policy_and_model_identity_guards_run_in_both_hook_events() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path().join("home");
    std::fs::create_dir_all(home.join("code/todo")).unwrap();
    let claude = home.join(".claude/CLAUDE.md");
    let record = home.join("code/todo/task.md");
    let cases: [(&str, Value, &str); 5] = [
        ("pre-edit", json!({"tool_name": "Write", "cwd": home, "tool_input": {"file_path": claude, "content": "x"}}), POLICY),
        ("pre-bash", json!({"tool_name": "Bash", "cwd": home, "tool_input": {"command": format!("echo x > {}", claude.display())}}), POLICY),
        ("pre-edit", json!({"tool_name": "Write", "cwd": home, "tool_input": {"file_path": record, "content": "model = inherited"}}), MODEL),
        ("pre-bash", json!({"tool_name": "Bash", "cwd": home, "tool_input": {"command": format!("echo 'model = self' > {}", record.display())}}), MODEL),
        ("pre-bash", json!({"tool_name": "functions.exec_command", "cwd": home, "tool_input": {"cmd": format!("echo 'model = self' > {}", record.display())}}), MODEL),
    ];
    for (kind, payload, reason) in &cases {
        let denied = run(kind, &home, payload, false);
        let decision: Value = serde_json::from_str(&denied).unwrap();
        let output = &decision["hookSpecificOutput"];
        assert_eq!(output["permissionDecision"], "deny", "{payload}: {denied}");
        assert!(output["permissionDecisionReason"].as_str().unwrap().contains(reason), "{payload}: {denied}");
        assert_eq!(run(kind, &home, payload, true), "", "{payload}");
    }
    let allowed = json!({"tool_name": "Write", "cwd": home, "tool_input": {"file_path": record, "content": "model = gpt-6-sol"}});
    assert_eq!(run("pre-edit", &home, &allowed, false), "");
}
