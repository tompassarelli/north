use super::*;
#[path = "launch_critical_fixtures.rs"]
mod fixtures;
use fixtures::fixture;
use patch::excise_sanctioned;
fn codex_equivalent(payload: &serde_json::Value) -> Option<serde_json::Value> {
    let command = payload["tool_input"]["command"].as_str()?;
    let mut codex = payload.clone();
    codex["tool_name"] = "exec_command".into();
    codex["tool_input"] = serde_json::json!({"cmd": command, "workdir": payload["tool_input"]["workdir"]});
    Some(codex)
}

#[test]
fn launch_guard_preserves_recorded_writes_and_allows_lanes() {
    if std::env::var_os("NORTH_LAUNCH_GUARD_CHILD").is_some() {
        crate::hook::run(&[]).unwrap();
        return;
    }
    let fixture = fixture();
    let policy = Policy {
        root: fixture.root.clone(),
        home: std::env::var("HOME").unwrap(),
    };
    assert!(
        policy
            .decide(&HookInput::parse(&fixtures::ordered_payload(&fixture.root)).unwrap())
            .is_some()
    );
    for (name, payload, denied) in &fixture.cases {
        let input = HookInput::parse(&serde_json::to_vec(payload).unwrap()).unwrap();
        let reason = policy.decide(&input);
        assert_eq!(reason.is_some(), *denied, "{name}: {reason:?}");
        if let Some(codex) = codex_equivalent(payload) {
            let codex = HookInput::parse(&serde_json::to_vec(&codex).unwrap()).unwrap();
            assert_eq!(policy.decide(&codex), reason, "{name}: exec_command");
        }
        if name.contains("/pins/") && *denied {
            assert!(reason.unwrap().contains("docs-boundary-consumer"), "{name}");
        }
    }
    let pin = format!(
        "{}/proj/pins/0123456789abcdef0123456789abcdef01234567",
        fixture.root
    );
    let payload = serde_json::json!({"tool_name":"Edit", "tool_input":{"file_path":format!("{pin}/index.html")}});
    let input = HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap();
    let before = policy.decide(&input).unwrap();
    std::fs::write(format!("{pin}.pin"), "a\r\n".repeat(2048)).unwrap();
    assert_eq!(
        policy.decide(&input).unwrap(),
        before.replace(
            "Consumers: docs-boundary-consumer.",
            &std::iter::repeat_n("a", 2048).collect::<Vec<_>>().join(" ")
        )
    );
    std::fs::write(format!("{pin}.pin"), "Consumers: docs-boundary-consumer.\n").unwrap();
    let active = fixture.directory.path().join("activation.active");
    std::fs::write(&active, "hook north-hook\n").unwrap();
    let errors = fixture.directory.path().join("errors.tsv");
    let main = format!("{}/proj/main", fixture.root);
    std::fs::write(format!("{main}/.gitignore"), "build/\n").unwrap();
    let protected = serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"Edit", "tool_input":{"file_path":format!("{main}/src/kept.txt")}});
    let root = fixture.directory.path().to_str().unwrap();
    let entry_cases = [
        (protected.clone(), Some("0"), true),
        (protected.clone(), Some("1"), false),
        (protected.clone(), None, true),
        (
            serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"Edit", "tool_input":{"file_path":format!("{main}/build/out.js")}}),
            Some("0"),
            false,
        ),
        (
            serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"Edit", "tool_input":{"file_path":format!("{pin}/build/out.js")}}),
            Some("0"),
            true,
        ),
        (
            serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"Bash", "tool_input":{"command":format!("CARGO_TARGET_DIR={}/proj/target-agent-a cargo test", fixture.root)}}),
            Some("0"),
            true,
        ),
        (
            serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"apply_patch", "cwd":root, "tool_input":{"input":"*** Begin Patch\n*** Update File: code/proj/main/src/kept.txt\n@@\n-a\n+b\n*** End Patch"}}),
            Some("0"),
            true,
        ),
        (
            serde_json::json!({"hook_event_name":"PreToolUse", "tool_name":"Edit", "tool_input":[format!("{main}/src/kept.txt")]}),
            Some("0"),
            false,
        ),
    ];
    for (payload, override_value, denied) in &entry_cases {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap());
        child.args(["--exact", "guard::launch_critical::tests::launch_guard_preserves_recorded_writes_and_allows_lanes", "--nocapture"])
            .env("NORTH_LAUNCH_GUARD_CHILD", "1").env("HOME", root)
            .env("NORTH_AGENT_ACTIVE", &active).env("NORTH_AGENT_ACTIVATION", fixture.directory.path().join("missing.json"))
            .env("AGENT_HOOK_ERRORS", &errors).env_remove("LAUNCH_CRITICAL_CODE_ROOT").env_remove("NORTH_HOOK_ID")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(value) = override_value {
            child.env("AGENT_NO_AUTHORING_HOOKS", value);
        } else {
            child.env_remove("AGENT_NO_AUTHORING_HOOKS");
        }
        let mut child = child.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(payload).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{payload}: {:?}", output.stderr);
        let stdout = String::from_utf8(output.stdout).unwrap();
        let decision = stdout
            .lines()
            .find_map(|line| serde_json::from_str::<Value>(line).ok());
        assert_eq!(decision.is_some(), *denied, "{payload}: {stdout}");
        if let Some(decision) = decision {
            let input = HookInput::parse(&serde_json::to_vec(payload).unwrap()).unwrap();
            assert_eq!(
                decision["hookSpecificOutput"]["permissionDecisionReason"],
                policy.decide(&input).unwrap()
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(&errors)
            .unwrap()
            .lines()
            .map(|line| line.split('\t').nth(2).unwrap())
            .collect::<Vec<_>>(),
        ["decider-exception"]
    );
    std::fs::write(&active, "hook other\n").unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact", "guard::launch_critical::tests::launch_guard_preserves_recorded_writes_and_allows_lanes", "--nocapture"])
        .env("NORTH_LAUNCH_GUARD_CHILD", "1").env("HOME", root).env("NORTH_AGENT_ACTIVE", &active)
        .env_remove("AGENT_NO_AUTHORING_HOOKS").env_remove("NORTH_HOOK_ID").env_remove("LAUNCH_CRITICAL_CODE_ROOT")
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&protected).unwrap())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .any(|line| serde_json::from_str::<Value>(line).is_ok())
    );
    println!(
        "launch guard: {} ported fixture/property and entry-point decisions passed",
        fixture.cases.len() + entry_cases.len() + 3
    );
}

#[test]
fn only_a_north_wt_rescue_segment_is_sanctioned_against_a_main_checkout() {
    assert_eq!(excise_sanctioned("north wt-rescue ~/code/p/main"), "");
    assert_eq!(excise_sanctioned("FOO=1 env north wt-rescue ~/code/p/main"), "");
    assert_eq!(excise_sanctioned("rm -rf main; north wt-rescue --dry-run"), "rm -rf main; ");
    assert_eq!(excise_sanctioned("wt-rescue ~/code/p/main"), "wt-rescue ~/code/p/main");
    assert_eq!(excise_sanctioned("north reclaim wt-rescue"), "north reclaim wt-rescue");
}

fn in_place_edit_from_main(command: impl FnOnce(&str) -> String) -> Option<String> {
    let fixture = fixture();
    let lane = format!("{}/wisp/worktrees/sc-812-cache", fixture.root);
    std::fs::create_dir_all(format!("{lane}/host/toolchain")).unwrap();
    std::fs::write(format!("{lane}/host/toolchain/package.ts"), "a\n").unwrap();
    let policy = Policy {
        root: fixture.root.clone(),
        home: std::env::var("HOME").unwrap(),
    };
    let payload = serde_json::json!({
        "tool_name": "Bash",
        "cwd": format!("{}/proj/main", fixture.root),
        "tool_input": {"command": command(&lane)},
    });
    policy.decide(&HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap())
}

#[test]
fn an_in_place_edit_after_cd_semicolon_into_another_repos_lane_is_allowed() {
    let reason = in_place_edit_from_main(|lane| {
        format!("cd {lane}; perl -0pi -e 's/a/b/' host/toolchain/package.ts")
    });
    assert_eq!(reason, None);
}

#[test]
fn the_same_in_place_edit_without_the_cd_is_refused_in_the_primary_checkout() {
    let reason = in_place_edit_from_main(|_| {
        "perl -0pi -e 's/a/b/' host/toolchain/package.ts".into()
    })
    .unwrap();
    assert!(reason.contains("edit in place"), "{reason}");
    assert!(reason.contains("host/toolchain/package.ts"), "{reason}");
}
