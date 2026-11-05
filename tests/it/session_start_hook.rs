use serde_json::{Value, json};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Rig {
    scratch: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        std::fs::write(root.join("activation.active"), "hook north-hook\n").unwrap();
        Self { scratch }
    }

    fn fleet(&self, status: &str) {
        let root = self.scratch.path();
        let nodes = json!([{"name": "holder", "conversation": "fixture-session", "task_ref": "makgora#10", "status": status}]);
        std::fs::write(root.join("fleet.json"), nodes.to_string()).unwrap();
        let stub = root.join("north-stub");
        north_core::foundation::shim::write_executable(&stub, format!("#!/bin/sh\ncat {}\n", root.join("fleet.json").display()));
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
            "cwd": self.scratch.path().join("home/code/makgora/main"),
        })
        .to_string()
    }
}

#[test]
fn session_start_lists_its_own_repositorys_stale_lanes() {
    let rig = Rig::new();
    rig.fleet("working");
    assert_eq!(rig.start(&rig.payload(), &[], None), None, "no lane report, so nothing to list");

    let report = rig.scratch.path().join("unlanded.txt");
    std::fs::write(&report, "48h\tmakgora\t(branch)\tfeature/resume-10\t+1\t-\tResume\t#10\tlost\n90h\twisp\t(branch)\tw\t+1\t-\tW\t#10\tlost\n").unwrap();
    let context = rig.start(&rig.payload(), &[("LANE_GC_UNLANDED", &report)], None).unwrap();
    assert!(context.starts_with("1 unlanded makgora lanes older than 24 h"), "{context}");
    assert!(context.contains("Lanes referencing makgora#10: feature/resume-10."), "{context}");
    assert!(!context.contains("wisp"), "{context}");
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
