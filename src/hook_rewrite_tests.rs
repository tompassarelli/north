use super::*;

#[test]
fn later_guards_preserve_and_inspect_prior_command_rewrites() {
    for bytes in [
        br#"{"tool_name":"Bash","tool_input":{"command":"original","timeout":1000}}"#.as_slice(),
        br#"{"tool_name":"functions.exec_command","tool_input":"{\"cmd\":\"original\",\"timeout\":1000}"}"#.as_slice(),
    ] {
        let input = HookInput::parse(bytes).unwrap();
        let wrap: Guard = |input| {
            let mut updated = input.tool_input.clone();
            let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
            updated[field] = format!("wrapped({})", input.command().unwrap().lossy()).into();
            Verdict::Rewrite(updated, "wrapped".into())
        };
        let output = evaluate(&input, &[wrap, wrap], "PreToolUse").unwrap();
        let updated = &output["hookSpecificOutput"]["updatedInput"];
        let field = if input.tool_name() == "Bash" { "command" } else { "cmd" };
        assert_eq!(updated[field].as_str(), Some("wrapped(wrapped(original))"));
        assert_eq!(updated["timeout"], input.tool_input["timeout"]);
        let deny_wrapped: Guard = |input| {
            assert_eq!(input.command().unwrap().lossy(), "wrapped(original)");
            Verdict::Deny("refuse transformed command".into())
        };
        let output = evaluate(&input, &[wrap, deny_wrapped], "PreToolUse").unwrap();
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"].as_str(), Some("deny"));
    }
}

const CHILD: &str = "NORTH_HOOK_REWRITE_CHILD";
const REWRITES: &[(&str, &str)] = &[
    ("grep -rn foo src", "rg -n foo src"),
    ("grep -rn foo", "rg -n foo ."),
    (
        "grep -rn -i hiveworkshop docs/references.md 'docs/*.md' '2>/dev/null'",
        "rg -ni hiveworkshop docs/references.md 'docs/*.md' '2>/dev/null'",
    ),
    (
        "grep -r foo $DIR 2>&1 | head -5",
        "rg foo $DIR 2>&1 | head -5",
    ),
    (
        "grep -rl foo src docs/*.md > /tmp/found.txt",
        "rg -l foo src docs/*.md > /tmp/found.txt",
    ),
    (
        "grep --include='*.rs' -rn foo src",
        "rg -n -g '*.rs' foo src",
    ),
    ("find src -name '*.rs'", "fd -e rs . src"),
    ("git push origin HEAD:main", "safe-push --to main"),
    ("cd x && git push -u origin HEAD:land-1 2>&1", "cd x && safe-push --to land-1 2>&1"),
    ("bun /tmp/task.mjs", "run-bounded 24h -- bun /tmp/task.mjs"),
    ("nohup bun run a > /tmp/a.log 2>&1 &", "run-bounded 24h -- bun run a > /tmp/a.log 2>&1 &"),
    ("A=1 nohup bun run a &", "run-bounded 24h -- env A=1 bun run a &"),
    ("(cd ts && nohup bun install > /tmp/i.log 2>&1 &)", "(cd ts && run-bounded 24h -- bun install > /tmp/i.log 2>&1 &)"),
    (
        "(cd /tmp && echo hi > log; echo \"exit $?\" >> log) & sleep 1",
        "run-bounded 24h -- bash -c 'cd /tmp && echo hi > log; echo \"exit $?\" >> log' & sleep 1",
    ),
    (
        "cat > /tmp/x.ts <<'EOF'\nconsole.log(1)\nEOF\nbun /tmp/x.ts",
        "cat > /tmp/x.ts <<'EOF'\nconsole.log(1)\nEOF\nrun-bounded 24h -- bun /tmp/x.ts",
    ),
    ("cd x && (bun a; bun b) &", "cd x && run-bounded 24h -- bash -c 'bun a; bun b' &"),
    ("grep -rn foo --include=*.rs src", "rg -n -g '*.rs' foo src"),
    ("cd /tmp && sleep 600 &", "cd /tmp && run-bounded 24h -- sleep 600 &"),
    (
        "FOO=1 bun run dev > /tmp/dev.log 2>&1 &",
        "run-bounded 24h -- env FOO=1 bun run dev > /tmp/dev.log 2>&1 &",
    ),
    (
        "bun run a & bun run b &",
        "run-bounded 24h -- bun run a & run-bounded 24h -- bun run b &",
    ),
];
const REFUSALS: &[&str] = &[
    "git push origin feature",
    "git push --force origin HEAD:main",
    "git push origin HEAD:refs/heads/main",
    "setsid bun task",
    "tail -f log | grep x &",
    "wait & disown",
];

fn pre_bash(command: &str, directory: &std::path::Path, active: &std::path::Path) -> Option<serde_json::Value> {
    use std::io::Write;
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": directory,
        "tool_input": {"command": command, "timeout": 1000}
    });
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "hook::rewrite_tests::pre_bash_rewrites_mechanical_shapes_and_keeps_refusals", "--nocapture"])
        .env(CHILD, "1")
        .env("HOME", directory)
        .env("NORTH_AGENT_ACTIVE", active)
        .env("NORTH_AGENT_ACTIVATION", directory.join("missing.json"))
        .env_remove("AGENT_NO_AUTHORING_HOOKS")
        .env_remove("NORTH_HOOK_ID")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
}

#[test]
fn pre_bash_rewrites_mechanical_shapes_and_keeps_refusals() {
    if std::env::var_os(CHILD).is_some() {
        crate::hook::run(&["pre-bash".into()]).unwrap();
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let active = directory.path().join("activation.active");
    std::fs::write(&active, "hook modern-search-guard\nhook session-kill-guard\nhook north-hook\n").unwrap();
    for (command, expected) in REWRITES {
        let output = pre_bash(command, directory.path(), &active)
            .unwrap_or_else(|| panic!("{command}: allowed unchanged"));
        let specific = &output["hookSpecificOutput"];
        assert_eq!(specific["permissionDecision"], "allow", "{command}: {output}");
        assert_eq!(specific["updatedInput"]["command"], *expected, "{command}");
        assert_eq!(specific["updatedInput"]["timeout"], 1000, "{command}");
    }
    for command in REFUSALS {
        let output = pre_bash(command, directory.path(), &active)
            .unwrap_or_else(|| panic!("{command}: allowed"));
        assert_eq!(output["hookSpecificOutput"]["permissionDecision"], "deny", "{command}: {output}");
    }
}

#[test]
fn commit_all_becomes_the_modified_tracked_paths_only_when_nothing_else_is_staged() {
    use crate::guard::commit_paths::rewrite;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-C"])
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(path.join("a.txt"), "a\n").unwrap();
    std::fs::write(path.join("b c.txt"), "b\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let cwd = path.to_str().unwrap();
    assert_eq!(rewrite("git commit -am 'fix it'", cwd), None);

    std::fs::write(path.join("a.txt"), "changed\n").unwrap();
    std::fs::remove_file(path.join("b c.txt")).unwrap();
    std::fs::write(path.join("new.txt"), "untracked\n").unwrap();
    let expected = "git commit -m 'fix it' -- a.txt 'b c.txt'";
    assert_eq!(rewrite("git commit -am 'fix it'", cwd).as_deref(), Some(expected));
    assert_eq!(rewrite("git commit -a -m 'fix it'", cwd).as_deref(), Some(expected));
    assert_eq!(
        rewrite("git add -u && git commit -m x -a", cwd).as_deref(),
        Some("git add -u && git commit -m x -- a.txt 'b c.txt'")
    );
    for refused in [
        "cd sub && git commit -am x",
        "git commit --amend -am x",
        "git commit -a",
        "git commit -am \"$(cat msg)\"",
        "git -C other commit -am x",
    ] {
        assert_eq!(rewrite(refused, cwd), None, "{refused}");
    }

    git(&["add", "a.txt"]);
    assert_eq!(rewrite("git commit -am x", cwd), None);
}
