use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn which(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("{name} not on PATH"))
}

struct Rig {
    root: tempfile::TempDir,
    repo: PathBuf,
    bare: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        let (tools, stub) = (base.join("tools"), base.join("stub"));
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::create_dir_all(&stub).unwrap();
        for name in ["git", "cat", "sleep", "sh"] {
            symlink(which(name), tools.join(name)).unwrap();
        }
        let script = stub.join("code-health");
        std::fs::write(
            &script,
            format!(
                "#!{}\n[ \"$1\" = check ] || exit 2\ncase \"$(cat \"$2\")\" in\n  *slow*) exec sleep 5 ;;\n  *orphan*) (sleep 30 & echo $! >\"$HOME/orphan.pid\"); echo orphan; exit 1 ;;\n  *broken*) echo 'internal error'; exit 2 ;;\n  *clean*) exit 0 ;;\nesac\necho \"file-size ${{2#$PWD/}}: 612 lines > 500; split the file\"\nexit 1\n",
                which("sh").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let (repo, bare) = (base.join("repo"), base.join("bare"));
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        for dir in [&repo, &bare] {
            assert!(Command::new("git").args(["init", "-q"]).arg(dir).status().unwrap().success());
        }
        std::fs::write(repo.join("structure.toml"), "[thresholds]\nfile_lines = 500\n").unwrap();
        for (dir, name) in [
            (&repo, "src/big.ts"),
            (&repo, "src/clean.rs"),
            (&repo, "src/slow.c"),
            (&repo, "src/broken.tsx"),
            (&repo, "src/orphan.c"),
            (&repo, "notes.md"),
            (&bare, "big.ts"),
        ] {
            let stem = Path::new(name).file_stem().unwrap().to_str().unwrap();
            std::fs::write(dir.join(name), format!("{stem}\n")).unwrap();
        }
        std::fs::write(base.join("activation.active"), "hook north-hook\n").unwrap();
        Self { root, repo, bare }
    }

    fn run(&self, payload: &str, env: &[(&str, &str)]) -> (std::process::Output, Duration) {
        let base = self.root.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["hook", "post-tool"])
            .env_clear()
            .env("HOME", base)
            .env("NORTH_AGENT_ACTIVE", base.join("activation.active"))
            .env("CODE_HEALTH_EDIT_TIMEOUT", "20")
            .env("PATH", format!("{}:{}", base.join("stub").display(), base.join("tools").display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            command.env(name, value);
        }
        let started = Instant::now();
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        (child.wait_with_output().unwrap(), started.elapsed())
    }
}

fn payload(tool: &str, input: Value, cwd: &Path) -> String {
    json!({
        "hook_event_name": "PostToolUse", "tool_name": tool, "cwd": cwd,
        "tool_input": input, "tool_response": {"success": true},
    })
    .to_string()
}

const PATCH: &str = "*** Begin Patch\n*** Update File: src/big.ts\n@@\n-a\n+b\n*** Add File: src/clean.rs\n+clean\n*** End Patch";

#[test]
fn an_edit_that_adds_structural_debt_is_reported_as_context() {
    let rig = Rig::new();
    let big = rig.repo.join("src/big.ts");
    let big = big.to_str().unwrap();
    let reported = [
        payload("Edit", json!({"file_path": big, "old_string": "a", "new_string": "b"}), &rig.repo),
        payload("Write", json!({"file_path": big, "content": "big"}), &rig.repo),
        payload("MultiEdit", json!({"file_path": big, "edits": [{"old_string": "a", "new_string": "b"}]}), &rig.repo),
        payload("Write", json!({"file_path": "src/big.ts", "content": "big"}), &rig.repo),
        payload("apply_patch", json!({"command": PATCH}), &rig.repo),
        payload("apply_patch", json!({"input": PATCH}), &rig.repo),
        payload("apply_patch", json!({"command": PATCH.lines().collect::<Vec<_>>()}), &rig.repo),
    ];
    for payload in &reported {
        let (output, _) = rig.run(payload, &[]);
        assert!(output.status.success());
        let response: Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("no context for {payload}: {}", String::from_utf8_lossy(&output.stderr)));
        let hook = &response["hookSpecificOutput"];
        let text = hook["additionalContext"].as_str().unwrap();
        assert_eq!(hook["hookEventName"], "PostToolUse");
        assert!(text.starts_with("code-health: this edit adds structural debt; fix it now"), "{text}");
        assert!(text.contains("program-craftsmanship"), "{text}");
        assert!(text.contains("file-size src/big.ts: 612 lines > 500; split the file"), "{text}");
        assert!(hook.get("permissionDecision").is_none());
    }
}

#[test]
fn every_other_edit_and_every_failure_is_silent() {
    let rig = Rig::new();
    let file = |name: &str| rig.repo.join(name).to_str().unwrap().to_owned();
    let big = payload("Edit", json!({"file_path": file("src/big.ts")}), &rig.repo);
    let tools = rig.root.path().join("tools");
    let tools = tools.to_str().unwrap();
    let silent = vec![
        ("clean edit", payload("Edit", json!({"file_path": file("src/clean.rs")}), &rig.repo), vec![]),
        ("no structure.toml", payload("Write", json!({"file_path": rig.bare.join("big.ts")}), &rig.bare), vec![]),
        ("non-source extension", payload("Write", json!({"file_path": file("notes.md")}), &rig.repo), vec![]),
        ("missing file", payload("Write", json!({"file_path": file("src/gone.ts")}), &rig.repo), vec![]),
        ("tool error", payload("Edit", json!({"file_path": file("src/broken.tsx")}), &rig.repo), vec![]),
        ("other tool", payload("Read", json!({"file_path": file("src/big.ts")}), &rig.repo), vec![]),
        ("malformed stdin", r#"{"tool_name":"Edit","tool_input":"#.to_owned(), vec![]),
        ("non-object stdin", r#"["Edit"]"#.to_owned(), vec![]),
        (
            "oversize stdin",
            payload("Write", json!({"file_path": file("src/big.ts"), "content": "x".repeat(1_100_000)}), &rig.repo),
            vec![],
        ),
        ("missing code-health", big.clone(), vec![("PATH", tools)]),
        ("override", big.clone(), vec![("AGENT_NO_AUTHORING_HOOKS", "1")]),
        ("inactive generation", big, vec![("NORTH_AGENT_ACTIVE", "/nonexistent/none")]),
    ];
    for (name, payload, env) in &silent {
        let (output, _) = rig.run(payload, env);
        assert!(output.status.success(), "{name}");
        assert!(output.stdout.is_empty(), "{name}: {}", String::from_utf8_lossy(&output.stdout));
        assert!(output.stderr.is_empty(), "{name}: {}", String::from_utf8_lossy(&output.stderr));
    }
}

#[test]
fn a_slow_checker_is_killed_at_the_limit_and_stays_silent() {
    let rig = Rig::new();
    let slow = payload("Edit", json!({"file_path": rig.repo.join("src/slow.c")}), &rig.repo);
    let (output, elapsed) = rig.run(&slow, &[("CODE_HEALTH_EDIT_TIMEOUT", "1")]);
    assert!(output.status.success() && output.stdout.is_empty());
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
}

fn alive(pid: &str) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .is_ok_and(|stat| !stat.rsplit(") ").next().unwrap_or("").starts_with('Z'))
}

#[test]
fn background_children_of_the_checker_are_killed_when_it_exits() {
    let rig = Rig::new();
    let orphan = payload("Edit", json!({"file_path": rig.repo.join("src/orphan.c")}), &rig.repo);
    let (output, _) = rig.run(&orphan, &[]);
    assert!(output.status.success());
    let pid = std::fs::read_to_string(rig.root.path().join("orphan.pid")).unwrap();
    let pid = pid.trim();
    let deadline = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive(pid), "background child {pid} outlived the checker");
}

#[test]
fn an_overflowing_timeout_falls_back_to_the_default_instead_of_panicking() {
    let rig = Rig::new();
    let big = payload("Edit", json!({"file_path": rig.repo.join("src/big.ts")}), &rig.repo);
    for value in ["1e20", "inf", "-3", "soon"] {
        let (output, _) = rig.run(&big, &[("CODE_HEALTH_EDIT_TIMEOUT", value)]);
        assert!(output.status.success() && output.stderr.is_empty(), "{value}: {}", String::from_utf8_lossy(&output.stderr));
    }
}
