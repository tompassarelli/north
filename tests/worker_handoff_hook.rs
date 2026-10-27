use serde_json::{Value, json};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Lab {
    root: tempfile::TempDir,
}

impl Lab {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("home")).unwrap();
        Self { root }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn activate(&self, name: &str, units: &str) -> PathBuf {
        let list = self.path(&format!("{name}.active"));
        std::fs::write(&list, units).unwrap();
        list
    }

    fn run(&self, payload: &[u8], active: &Path, extra: &[(&str, String)]) -> Option<Value> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["hook", "post-tool"])
            .env_clear()
            .env("HOME", self.path("home"))
            .env("PATH", self.path("bin"))
            .env("NORTH_AGENT_ACTIVE", active)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in extra {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
        serde_json::from_slice(&output.stdout).ok()
    }

    fn stub_threads(&self) {
        let bin = self.path("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let cat = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("cat"))
            .find(|path| path.is_file())
            .expect("cat on PATH");
        std::os::unix::fs::symlink(cat, bin.join("cat")).unwrap();
        let script = bin.join("threads");
        std::fs::write(&script, "#!/bin/sh\ncat > \"$STUB_OUT\"\nexit \"${STUB_EXIT:-0}\"\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn worker_payload(lab: &Lab) -> Value {
    let subagents = lab.path("projects/s/subagents");
    std::fs::create_dir_all(&subagents).unwrap();
    let line = json!({"type": "assistant", "message": {"usage": {"input_tokens": 400_000}}});
    std::fs::write(subagents.join("agent-w1.jsonl"), format!("{line}\n")).unwrap();
    json!({
        "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "true"},
        "agent_id": "w1", "agent_type": "worker", "transcript_path": lab.path("projects/s.jsonl"),
    })
}

fn context(response: &Option<Value>) -> Option<&str> {
    let output = response.as_ref()?;
    assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    output["hookSpecificOutput"]["additionalContext"].as_str()
}

#[test]
fn worker_handoff_follows_its_hook_activation() {
    let lab = Lab::new();
    let payload = worker_payload(&lab).to_string();
    let other = lab.activate("other", "hook unrelated-guard\n");
    let missing = lab.path("missing.active");
    let active = lab.activate("on", "hook north-hook\n");
    let told = lab.run(payload.as_bytes(), &active, &[]);
    assert!(context(&told).is_some_and(|text| text.starts_with("Your context is 400k tokens, past the 350k handoff point.")));
    std::fs::remove_dir_all(lab.path("home/.local")).unwrap();
    assert_eq!(lab.run(payload.as_bytes(), &active, &[("AGENT_NO_AUTHORING_HOOKS", "1".into())]), None);
    assert_eq!(lab.run(payload.as_bytes(), &other, &[]), None);
    assert_eq!(lab.run(payload.as_bytes(), &missing, &[]), None);
    assert!(!lab.path("home/.local").exists());
    assert_eq!(lab.run(br#"{"tool_name":"#, &active, &[]), None);
    assert!(lab.run(payload.as_bytes(), &active, &[("AGENT_NO_AUTHORING_HOOKS", "0".into())]).is_some());
}

fn spawn_payload(response: Value) -> Value {
    let brief = format!("Delegation: role=worker\nGoal: keep  spacing\n{}", "x".repeat(4200));
    json!({
        "session_id": "claude-lead", "hook_event_name": "PostToolUse", "tool_name": "Agent", "tool_use_id": "toolu_9",
        "tool_input": {"subagent_type": "worker-high", "prompt": brief, "run_in_background": true},
        "tool_response": response,
    })
}

#[test]
fn worker_handoff_records_async_worker_spawns() {
    let lab = Lab::new();
    lab.stub_threads();
    let active = lab.activate("on", "hook north-hook\n");
    let sink = lab.path("ingested.json");
    let env = |exit: &str| vec![("STUB_OUT", sink.display().to_string()), ("STUB_EXIT", exit.to_owned())];
    let launched = json!({"status": "async_launched", "agentId": "actual-child", "resolvedModel": "claude-opus-4-6"});
    let payload = spawn_payload(launched.clone());
    assert_eq!(lab.run(payload.to_string().as_bytes(), &active, &env("0")), None);
    let text = std::fs::read_to_string(&sink).unwrap();
    assert!(text.ends_with('\n'));
    let row: Value = serde_json::from_str(&text).unwrap();
    let brief = payload["tool_input"]["prompt"].clone();
    assert_eq!(row["id"], "actual-child");
    assert_eq!(row["session"], "claude-lead");
    assert_eq!((row["status"].as_str(), row["role"].as_str(), row["tier"].as_str()), (Some("running"), Some("worker"), Some("high")));
    assert_eq!(row["model"], "claude-opus-4-6");
    assert_eq!((&row["brief"], &row["brief_text"]), (&brief, &brief));
    assert_eq!(row["brief_origin"], "claude.PostToolUse.Agent");
    assert_eq!(row["started"], row["staffed"]);

    std::fs::remove_file(&sink).unwrap();
    for response in [json!({"status": "async_launched"}), json!({"status": "completed", "agentId": "finished-child"})] {
        let skipped = spawn_payload(response);
        assert_eq!(lab.run(skipped.to_string().as_bytes(), &active, &env("0")), None);
    }
    let mut plain = spawn_payload(launched.clone());
    plain["tool_input"]["subagent_type"] = json!("worker");
    assert_eq!(lab.run(plain.to_string().as_bytes(), &active, &env("0")), None);
    assert_eq!(serde_json::from_str::<Value>(&std::fs::read_to_string(&sink).unwrap()).unwrap()["tier"], "medium");

    std::fs::remove_file(&sink).unwrap();
    assert_eq!(lab.run(payload.to_string().as_bytes(), &active, &[("AGENT_NO_AUTHORING_HOOKS", "1".into()), env("0").remove(0)]), None);
    assert!(!sink.exists());

    let failed = lab.run(payload.to_string().as_bytes(), &active, &env("3"));
    assert_eq!(context(&failed), Some("spawn recording boundary miss for lead claude-lead: CalledProcessError"));
    let mut led = payload.clone();
    led["agent_id"] = json!("lead-agent");
    let failed = lab.run(led.to_string().as_bytes(), &active, &env("3"));
    assert_eq!(context(&failed), Some("spawn recording boundary miss for lead lead-agent: CalledProcessError"));
    std::fs::remove_file(lab.path("bin/threads")).unwrap();
    let missing = lab.run(payload.to_string().as_bytes(), &active, &[]);
    assert_eq!(context(&missing), Some("spawn recording boundary miss for lead claude-lead: FileNotFoundError"));
}

#[test]
fn worker_handoff_confirms_the_spawn_reservation() {
    let lab = Lab::new();
    lab.stub_threads();
    let active = lab.activate("on", "hook north-hook\n");
    let agents = lab.path("capacity/.local/state/agents");
    std::fs::create_dir_all(&agents).unwrap();
    let state = json!({"grants": [{"account": "claude:x", "lead": "lead", "session": "claude-lead", "uses": 2,
        "pending": [{"at": 1.0, "id": "toolu_9"}, {"at": 1.0, "id": "toolu_10"}]}]});
    std::fs::write(agents.join("pace-control.json"), state.to_string()).unwrap();
    let env = vec![("SPAWN_CAPACITY_HOME", lab.path("capacity").display().to_string()), ("STUB_OUT", lab.path("sink").display().to_string())];
    let payload = spawn_payload(json!({"status": "completed", "agentId": "child"}));
    assert_eq!(lab.run(payload.to_string().as_bytes(), &active, &[("AGENT_NO_AUTHORING_HOOKS", "1".into()), env[0].clone()]), None);
    let before: Value = serde_json::from_slice(&std::fs::read(agents.join("pace-control.json")).unwrap()).unwrap();
    assert_eq!(before["grants"][0]["uses"], 2);
    assert_eq!(lab.run(payload.to_string().as_bytes(), &active, &env), None);
    let after: Value = serde_json::from_slice(&std::fs::read(agents.join("pace-control.json")).unwrap()).unwrap();
    assert_eq!(after["grants"][0]["uses"], 1);
    assert_eq!(after["grants"][0]["pending"][0]["confirmed"], true);
    assert_ne!(after["grants"][0]["pending"][1]["confirmed"], true);
    assert!(!lab.path("sink").exists());

    let locked = lab.path("locked/.local/state/agents");
    std::fs::create_dir_all(locked.join("pace-control.lock")).unwrap();
    std::fs::write(locked.join("pace-control.json"), state.to_string()).unwrap();
    let broken = vec![("SPAWN_CAPACITY_HOME", lab.path("locked").display().to_string()), env[1].clone()];
    let failed = lab.run(payload.to_string().as_bytes(), &active, &broken);
    assert!(context(&failed).is_some_and(|text| text.starts_with("spawn reservation confirmation failed for lead claude-lead: ")));
    assert!(!lab.path("sink").exists());
}
