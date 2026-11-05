#![cfg(test)]

use crate::guard::spawn_capacity::{Event, World, decide, launch_refusal};
use crate::foundation::clock::Timestamp;
use crate::hook::HookInput;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

struct Rig {
    dir: tempfile::TempDir,
    now: f64,
    probe: RefCell<Option<String>>,
    fleet: RefCell<String>,
    flow: RefCell<Option<Value>>,
    billed: RefCell<Option<String>>,
}

impl World for Rig {
    fn now(&self) -> f64 {
        self.now + 1.0
    }

    fn var(&self, name: &str) -> Option<String> {
        let path = match name {
            "HOME" | "SPAWN_CAPACITY_HOME" => self.home(),
            "XDG_STATE_HOME" => self.home().join(".local/state"),
            "CODEX_BEHAVIOR_STATE" => self.dir.path().join("state"),
            "AGENTS_ORCHESTRATION" => self.dir.path().join("orchestration.toml"),
            _ => return None,
        };
        Some(path.display().to_string())
    }

    fn probe(&self) -> Option<String> {
        self.probe.borrow().clone()
    }

    fn fleet(&self) -> Option<String> {
        Some(self.fleet.borrow().clone())
    }

    fn flow(&self, repo: &str) -> Option<String> {
        let row = self.flow.borrow().clone().filter(|_| repo == "makgora")?;
        let line = format!(
            "{}: pending {}, landed {}/h over 3h, push-to-land median {}m p90 {}m over 24h",
            row["repo"].as_str()?, row["pending"], row["landed_per_hour"], row["push_to_land_minutes"], row["push_to_land_p90_minutes"]
        );
        Some(json!({"row": row, "line": line}).to_string())
    }

    fn refresh(&self, _provider: &str) -> Option<(i32, String)> {
        Some((1, String::new()))
    }

    fn pace(&self, request: &Value) -> Result<Value, String> {
        crate::pacing::answer(request)
    }

    fn billed(&self, provider: &str) -> Option<String> {
        self.billed.borrow().clone().filter(|_| provider == "claude")
    }

    fn cpus(&self) -> f64 {
        20.0
    }

    fn today(&self) -> String {
        "2026/11/04".into()
    }
}

impl Rig {
    fn new() -> Self {
        let rig = Rig {
            dir: tempfile::tempdir().unwrap(),
            now: Timestamp::now().unix_seconds_f64(),
            probe: RefCell::new(None),
            fleet: RefCell::new("[]".into()),
            flow: RefCell::new(None),
            billed: RefCell::new(None),
        };
        let subagents = rig.home().join(".claude/projects/p/s/subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        for name in ["agent-a", "agent-b"] {
            std::fs::write(subagents.join(format!("{name}.jsonl")), "").unwrap();
        }
        rig.gate(Value::Null, json!({}));
        rig
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn agents(&self) -> PathBuf {
        self.home().join(".local/state/agents")
    }

    fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn gate(&self, refusal: Value, refusals: Value) {
        let fresh = |refusal: Value, refusals: Value| json!({"epoch": self.now, "stale_min": 30, "refusal": refusal, "refusals": refusals});
        self.write(&self.agents().join("usage-gate.json"), &json!({"claude": fresh(refusal, refusals), "codex": fresh(Value::Null, json!({}))}).to_string());
    }

    fn status(&self, leased: f64, protected: f64, system: f64) {
        self.raw_probe(&json!({"decision": "RUN", "committedBatchCpus": leased, "leasedNativeCpus": 12, "aggregateCpuLimit": 20, "protectedCpuSomeAvg10": protected, "cpuSomeAvg10": system}).to_string());
    }

    fn raw_probe(&self, text: &str) {
        *self.probe.borrow_mut() = Some(text.into());
    }

    fn said(&self, event: &Value) -> Option<String> {
        let input = HookInput::parse(event.to_string().as_bytes()).expect("hook input parses");
        decide(self, &Event::from_hook(&input).unwrap())
    }

}

fn agent(input: Value) -> Value {
    json!({"session_id": "s", "cwd": "/tmp", "hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": input})
}

fn worker() -> Value {
    agent(json!({"subagent_type": "worker", "description": "d", "prompt": "do it"}))
}

#[track_caller]
fn denies(rig: &Rig, event: &Value, case: &str) -> String {
    rig.said(event).unwrap_or_else(|| panic!("{case}: allowed"))
}

#[test]
fn a_local_fleet_launch_meets_the_spawn_gate_cpu_line_and_its_remedy() {
    let rig = Rig::new();
    rig.status(19.0, 5.0, 1.0);
    assert_eq!(launch_refusal(&rig, "go"), None, "19 of 20 committed CPUs admits a launch");
    rig.status(4.0, 20.0, 1.0);
    assert_eq!(launch_refusal(&rig, "go"), None, "protected pressure exactly 20 admits");
    rig.status(20.0, 5.0, 1.0);
    let refused = launch_refusal(&rig, "go").expect("committed CPUs at the limit refuse");
    assert_eq!(refused, denies(&rig, &worker(), "the spawn gate refuses the same host"), "the launch carries the spawn gate's remedy text");
    rig.status(4.0, 20.5, 1.0);
    let refused = launch_refusal(&rig, "go").expect("protected pressure over 20 refuses");
    assert!(refused.contains("commit 4 of the 20 CPUs") && refused.contains("pressure is 20.5%") && refused.contains("cloud worker"), "{refused}");
    rig.status(24.0, 5.0, 1.0);
    assert_eq!(launch_refusal(&rig, "CASE=URGENT FACT=\"prod login is down for every user\"\ngo"), None, "an urgent fact passes as it does for a spawn");
    assert!(launch_refusal(&rig, "CASE=URGENT FACT=\"short\"").is_some(), "a fact under 15 characters does not");
    rig.raw_probe("not json");
    assert_eq!(launch_refusal(&rig, "go"), None, "an unreadable probe admits");
}
