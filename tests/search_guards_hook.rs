use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Setup<'a> {
    home: &'a Path,
    activation: &'a Path,
    disabled: &'a str,
}

fn run(setup: &Setup, stdin: &[u8]) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["hook", "pre-bash"])
        .env_remove("NORTH_HOOK_ID")
        .env("HOME", setup.home)
        .env("NORTH_AGENT_ACTIVE", setup.activation)
        .env("AGENT_NO_AUTHORING_HOOKS", setup.disabled)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or(Value::Null)
}

fn call_in(setup: &Setup, cwd: &Path, tool_input: &Value) -> Value {
    let payload = json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": cwd,
        "tool_input": tool_input,
    });
    run(setup, payload.to_string().as_bytes())["hookSpecificOutput"].clone()
}

fn call(setup: &Setup, tool_input: &Value) -> Value {
    call_in(setup, Path::new("/tmp"), tool_input)
}

fn denies(setup: &Setup, command: &str) -> bool {
    call(setup, &json!({"command": command}))["permissionDecision"] == "deny"
}

fn scratch_home() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(scratch.path().join("home/code/north-data/accounts")).unwrap();
    scratch
}

#[test]
fn search_guards_follow_their_hook_activation() {
    let scratch = scratch_home();
    let home = scratch.path().join("home");
    let guards = [
        ("modern-search-guard", "grep -rn foo src".to_owned()),
        (
            "corpus-scan-guard",
            format!("CHECK=1 rg needle {}/code/north-data", home.display()),
        ),
        (
            "resource-safe-search-guard",
            "env FOO=1 rg TARGET /proc".to_owned(),
        ),
    ];
    for (unit, command) in &guards {
        let active = scratch.path().join(format!("{unit}.active"));
        std::fs::write(&active, format!("hook {unit}\n")).unwrap();
        let other = scratch.path().join(format!("{unit}.other"));
        std::fs::write(&other, "hook unrelated-guard\n").unwrap();
        let missing = scratch.path().join("missing.active");
        for (activation, disabled, expected) in [
            (&active, "", true),
            (&active, "0", true),
            (&active, "1", false),
            (&other, "", false),
            (&missing, "", false),
        ] {
            let setup = Setup {
                home: &home,
                activation,
                disabled,
            };
            assert_eq!(
                denies(&setup, command),
                expected,
                "{unit} with {} disabled={disabled:?}",
                activation.display()
            );
        }
    }
}

#[test]
fn modern_search_rewrites_pattern_file_greps_and_keeps_every_other_tool_input_field() {
    let scratch = scratch_home();
    let home = scratch.path().join("home");
    let active = scratch.path().join("a.active");
    std::fs::write(&active, "hook modern-search-guard\n").unwrap();
    let setup = Setup {
        home: &home,
        activation: &active,
        disabled: "0",
    };
    let source = "/tmp/source file.json";
    let command = format!("grep -qFf <(jq -r '.value' '{source}') '{source}'");
    let tool_input = json!({
        "command": command,
        "timeout": 1000,
        "description": "Search explicit JSON",
        "run_in_background": false,
        "provider_extra": {"preserve": [1, 2]},
    });
    let output = call(&setup, &tool_input);
    let mut expected = tool_input.clone();
    expected["command"] = json!(command.replacen("grep -qFf", "rg -qF -f", 1));
    assert_eq!(output["permissionDecision"], "allow");
    assert_eq!(output["updatedInput"], expected);
    assert!(
        output["additionalContext"]
            .as_str()
            .unwrap()
            .contains("modern-search-guard")
    );

    let still_refused = call(
        &setup,
        &json!({"command": format!("{command}; grep -r needle src")}),
    );
    assert_eq!(still_refused["permissionDecision"], "deny");
    assert!(still_refused.get("updatedInput").is_none());
}

#[test]
fn search_guards_judge_the_command_as_typed_before_any_guard_rewrites_it() {
    let scratch = scratch_home();
    let home = scratch.path().join("home");
    let active = scratch.path().join("all.active");
    std::fs::write(
        &active,
        "hook modern-search-guard\nhook corpus-scan-guard\nhook resource-safe-search-guard\nhook heavy-command-guard\n",
    )
    .unwrap();
    let setup = Setup {
        home: &home,
        activation: &active,
        disabled: "0",
    };
    for command in [
        "cargo test; grep -rn foo src".to_owned(),
        format!("cargo test && rg -l x {}/code/north-data", home.display()),
        "cargo test; rg x /proc".to_owned(),
    ] {
        assert!(denies(&setup, &command), "{command}");
    }

    let container = scratch.path().join("home/code/proj");
    std::fs::create_dir_all(container.join("main")).unwrap();
    std::fs::create_dir_all(container.join("worktrees")).unwrap();
    let command = "grep -qFf <(jq -r '.value' 'f.json') 'f.json'";
    let output = call_in(&setup, &container, &json!({"command": command}));
    assert_eq!(output["permissionDecision"], "allow");
    assert_eq!(
        output["updatedInput"]["command"],
        "rg -qF -f <(jq -r '.value' 'f.json') 'f.json'"
    );
}

#[test]
fn a_search_rewrite_and_a_later_capacity_rewrite_both_reach_the_updated_input() {
    let scratch = scratch_home();
    let home = scratch.path().join("home");
    let active = scratch.path().join("both.active");
    std::fs::write(
        &active,
        "hook modern-search-guard\nhook heavy-command-guard\n",
    )
    .unwrap();
    let setup = Setup {
        home: &home,
        activation: &active,
        disabled: "0",
    };
    let command = "grep -qFf <(jq -r '.value' 'f.json') 'f.json'; cargo test";
    let output = call(&setup, &json!({"command": command}));
    let updated = output["updatedInput"]["command"].as_str().unwrap();
    assert!(updated.contains("machine-capacity.mjs"), "{updated}");
    assert!(updated.contains("rg -qF -f"), "{updated}");
}
