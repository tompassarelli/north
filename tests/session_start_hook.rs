use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-C"])
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

struct Rig {
    scratch: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let origin = root.join("origin.git");
        let seed = root.join("seed");
        let clone = root.join("home/code/smashcraft/main");
        std::fs::create_dir_all(clone.parent().unwrap()).unwrap();
        git(root, &["init", "-q", "--bare", "--initial-branch=main", origin.to_str().unwrap()]);
        git(root, &["init", "-q", "--initial-branch=main", seed.to_str().unwrap()]);
        git(&seed, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "base"]);
        git(&seed, &["push", "-q", "origin", "main"]);
        git(&seed, &["checkout", "-qb", "feature/resume-10"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "Resume fixture\n\nRefs smashcraft#10"]);
        git(&seed, &["tag", "archive/feature/resume-10"]);
        git(&seed, &["push", "-q", "origin", "refs/tags/archive/feature/resume-10"]);
        git(&seed, &["checkout", "-q", "main"]);
        git(&seed, &["checkout", "-qb", "feature/other-10"]);
        git(&seed, &["commit", "-q", "--allow-empty", "-m", "Other repository fixture\n\nRefs wisp#10"]);
        git(&seed, &["tag", "-a", "archive/feature/other-10", "-m", "archived fixture"]);
        git(&seed, &["push", "-q", "origin", "refs/tags/archive/feature/other-10"]);
        git(root, &["clone", "-q", "--no-tags", origin.to_str().unwrap(), clone.to_str().unwrap()]);
        std::fs::write(root.join("activation.active"), "hook north-hook\n").unwrap();
        Self { scratch }
    }

    fn fleet(&self, status: &str) {
        let root = self.scratch.path();
        let nodes = json!([{"name": "holder", "conversation": "fixture-session", "task_ref": "smashcraft#10", "status": status}]);
        std::fs::write(root.join("fleet.json"), nodes.to_string()).unwrap();
        let stub = root.join("north-stub");
        std::fs::write(&stub, format!("#!/bin/sh\ncat {}\n", root.join("fleet.json").display())).unwrap();
        Command::new("chmod").arg("+x").arg(&stub).status().unwrap();
    }

    fn start(&self, payload: &str, extra: &[(&str, &Path)], disabled: Option<&str>) -> Option<String> {
        let root = self.scratch.path();
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["hook", "session-start"])
            .env("HOME", root.join("home"))
            .env("NORTH_AGENT_ACTIVE", root.join("activation.active"))
            .env("NORTH_BIN", root.join("north-stub"))
            .env_remove("LANE_GC_UNLANDED")
            .env_remove("AGENT_NO_AUTHORING_HOOKS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some(value) = disabled {
            command.env("AGENT_NO_AUTHORING_HOOKS", value);
        }
        for (name, value) in extra {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        let response: Value = serde_json::from_slice(&output.stdout).ok()?;
        assert_eq!(response["hookSpecificOutput"]["hookEventName"], "SessionStart");
        response["hookSpecificOutput"]["additionalContext"].as_str().map(str::to_owned)
    }

    fn payload(&self) -> String {
        json!({
            "hook_event_name": "SessionStart",
            "session_id": "fixture-session",
            "cwd": self.scratch.path().join("home/code/smashcraft/main"),
        })
        .to_string()
    }
}

#[test]
fn session_start_lists_the_holders_archives_and_the_stale_lane_count() {
    let rig = Rig::new();
    rig.fleet("working");
    let context = rig.start(&rig.payload(), &[], None).unwrap();
    assert!(context.contains("  archived: archive/feature/resume-10\n"), "{context}");
    assert!(
        context.contains("    restore: git push origin archive/feature/resume-10:refs/heads/feature/resume-10"),
        "{context}"
    );
    assert!(!context.contains("archive/feature/other-10"), "{context}");

    let report = rig.scratch.path().join("unlanded.txt");
    std::fs::write(&report, "48h\tsmashcraft\tfixture\tfeature/resume-10\n").unwrap();
    let context = rig.start(&rig.payload(), &[("LANE_GC_UNLANDED", &report)], None).unwrap();
    assert!(context.starts_with("1 unlanded worktrees/branches older than 24 h"), "{context}");
    assert!(context.contains("archived: archive/feature/resume-10"), "{context}");
}

#[test]
fn session_start_is_silent_when_off_exited_or_unreadable() {
    let rig = Rig::new();
    rig.fleet("working");
    assert_eq!(rig.start(&rig.payload(), &[], Some("1")), None);
    assert_eq!(rig.start("{not json", &[], None), None);
    rig.fleet("exited");
    assert_eq!(rig.start(&rig.payload(), &[], None), None);
}
