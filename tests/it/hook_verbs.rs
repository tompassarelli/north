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
    let mut command = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
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
    String::from_utf8(output.stdout).unwrap().replace(home.to_str().unwrap(), "{home}")
}

fn operator_login() -> bool {
    let named = match std::fs::read_to_string("/etc/north/operator") {
        Ok(text) => text.trim().to_owned(),
        Err(error) => return error.kind() == std::io::ErrorKind::NotFound,
    };
    let uid = std::fs::read_to_string("/etc/passwd").ok().and_then(|users| {
        users.lines().find_map(|line| {
            let mut fields = line.split(':');
            (fields.next() == Some(named.as_str())).then(|| fields.nth(1)?.parse::<u32>().ok()).flatten()
        })
    });
    uid == Some(rustix::process::getuid().as_raw())
}

#[test]
fn language_guard_denies_new_python_only_while_its_unit_is_active() {
    let payload = r#"{"tool_name":"Write","tool_input":{"file_path":"{home}/code/project/new.py","content":"print(1)"}}"#;
    let denied = run_hook("language-guard", "hook language-guard\n", payload, &[]);
    assert!(denied.contains("\"permissionDecision\":\"deny\""), "{denied}");
    let inactive = run_hook("language-guard", "hook north-hook\n", payload, &[]);
    if operator_login() {
        assert_eq!(inactive, "", "the operator login's inactive unit gives no output");
    } else {
        assert_eq!(inactive, denied, "a login the guard does not believe keeps the unit answering whatever the list says");
    }
}

#[test]
fn firn_system_policy_relays_the_policy_answer_only_while_its_unit_is_active() {
    let directory = tempfile::tempdir().unwrap();
    let policy = directory.path().join("policy");
    shim::write_script(&policy, "cat\n");
    let env = [("FIRN_SYSTEM_POLICY", policy.as_path())];
    let relayed = run_hook("firn-system-policy", "hook firn-system-policy\n", "{\"a\":1}", &env);
    assert_eq!(relayed, "{\"a\":1}");
    let inactive = run_hook("firn-system-policy", "hook north-hook\n", "{\"a\":1}", &env);
    if operator_login() {
        assert_eq!(inactive, "", "the operator login's inactive unit gives no output");
    } else {
        assert_eq!(inactive, relayed, "a login the guard does not believe keeps the unit answering whatever the list says");
    }
}
