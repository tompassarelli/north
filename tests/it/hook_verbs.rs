use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use north_core::foundation::shim;

fn run_hook(verb: &str, active: &str, payload: &str, env: &[(&str, &Path)]) -> String {
    let root = tempfile::tempdir().unwrap();
    let list = root.path().join("activation.active");
    std::fs::write(&list, active).unwrap();
    let home = root.path().join("home");
    let project = home.join("code/project");
    std::fs::create_dir_all(&project).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
    command
        .args(["hook", verb])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("NORTH_AGENT_ACTIVE", &list)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    for (name, value) in env {
        command.env(name, value);
    }
    let mut child = command.spawn().unwrap();
    let payload = payload.replace("{home}", home.to_str().unwrap());
    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn language_guard_denies_new_python_only_while_its_unit_is_active() {
    let payload = r#"{"tool_name":"Write","tool_input":{"file_path":"{home}/code/project/new.py","content":"print(1)"}}"#;
    let denied = run_hook("language-guard", "hook language-guard\n", payload, &[]);
    assert!(denied.contains("\"permissionDecision\":\"deny\""), "{denied}");
    assert_eq!(run_hook("language-guard", "hook north-hook\n", payload, &[]), "");
}

#[test]
fn firn_system_policy_relays_the_policy_answer_only_while_its_unit_is_active() {
    let directory = tempfile::tempdir().unwrap();
    let policy = directory.path().join("policy");
    shim::write_script(&policy, "cat\n");
    let env = [("FIRN_SYSTEM_POLICY", policy.as_path())];
    assert_eq!(run_hook("firn-system-policy", "hook firn-system-policy\n", "{\"a\":1}", &env), "{\"a\":1}");
    assert_eq!(run_hook("firn-system-policy", "hook north-hook\n", "{\"a\":1}", &env), "");
}
