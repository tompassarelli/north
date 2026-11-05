use super::spawn_capacity::{World, decide, gate_summary};
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
        crate::ops::pacing::answer(request)
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

    fn orchestration(&self, text: &str) {
        self.write(&self.dir.path().join("orchestration.toml"), text);
        *self.billed.borrow_mut() = Some("claude:a".into());
    }

    fn fleet_task(&self, task: &str) {
        *self.fleet.borrow_mut() = json!([{"name": "mu", "conversation": "s", "task_ref": task, "status": "working"}]).to_string();
    }

    fn flow_status(&self, pending: u64, landed: u64) {
        *self.flow.borrow_mut() = Some(json!({"repo": "makgora", "epoch": self.now, "pending": pending, "landed_per_hour": landed, "push_to_land_minutes": 12, "push_to_land_p90_minutes": 20}));
    }

    fn said(&self, event: &Value) -> Option<String> {
        let input = HookInput::parse(event.to_string().as_bytes()).expect("hook input parses");
        decide(self, &super::spawn_capacity::Event::from_hook(&input).unwrap())
    }

    fn file(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }
}

fn agent(input: Value) -> Value {
    json!({"session_id": "s", "cwd": "/tmp", "hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": input})
}

fn native(tool: &str, input: Value, cwd: &str) -> Value {
    json!({"session_id": "s", "cwd": cwd, "hook_event_name": "PreToolUse", "tool_name": tool, "tool_input": input})
}

fn worker() -> Value {
    agent(json!({"subagent_type": "worker", "description": "d", "prompt": "do it"}))
}

fn brief(prompt: &str) -> Value {
    agent(json!({"subagent_type": "worker", "prompt": prompt}))
}

#[track_caller]
fn allows(rig: &Rig, event: &Value, case: &str) {
    assert_eq!(rig.said(event), None, "{case}");
}

#[track_caller]
fn denies(rig: &Rig, event: &Value, case: &str) -> String {
    rig.said(event).unwrap_or_else(|| panic!("{case}: allowed"))
}

#[test]
fn provisioned_cpus_and_protected_pressure_decide_a_local_worker() {
    let rig = Rig::new();
    rig.status(19.0, 5.0, 1.0);
    allows(&rig, &worker(), "19 of 20 provisioned CPUs allows a local worker");
    rig.status(2.0, 5.0, 95.0);
    allows(&rig, &worker(), "system PSI 95 with 2 provisioned CPUs allows a local worker");
    rig.status(20.0, 5.0, 1.0);
    let message = denies(&rig, &worker(), "20 of 20 provisioned CPUs (at the limit) denies with the message");
    assert!(message.starts_with("Capacity leases and unleased heavy load commit 20 of the 20 CPUs the machine can hand out, protected desktop pressure is 5% (limit 20%), and 2 local workers are running."), "{message}");
    rig.status(4.0, 20.5, 1.0);
    let message = denies(&rig, &worker(), "protected pressure 20.5 denies with few provisioned CPUs");
    assert!(message.contains("commit 4 of the 20 CPUs") && message.contains("pressure is 20.5%"), "{message}");
    rig.status(4.0, 20.0, 1.0);
    allows(&rig, &worker(), "protected pressure exactly 20 allows");
    rig.raw_probe(r#"{"decision":"RUN","leasedBatchCpus":10,"committedBatchCpus":21,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":5}"#);
    assert!(denies(&rig, &worker(), "measured lease use plus unleased heavy load past the limit denies").contains("commit 21 of the 20"));
    rig.raw_probe(r#"{"decision":"RUN","leasedBatchCpus":24,"committedBatchCpus":19,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":5}"#);
    allows(&rig, &worker(), "committed CPUs, when reported, replace reserved ones");
    rig.raw_probe(r#"{"decision":"RUN","profile":"unattended","committedBatchCpus":4,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":40}"#);
    allows(&rig, &worker(), "unattended profile (Tom away) skips the protected-pressure refusal, as the helper does");
    rig.raw_probe(r#"{"decision":"RUN","profile":"unattended","committedBatchCpus":20,"aggregateCpuLimit":20,"protectedCpuSomeAvg10":40}"#);
    assert!(denies(&rig, &worker(), "unattended profile still denies at the provisioned limit").contains("commit 20 of the 20 CPUs the machine can hand out, protected desktop pressure is 0%"));
    rig.status(24.0, 5.0, 1.0);
    denies(&rig, &worker(), "24 of 20 provisioned CPUs (over the limit) denies with the message");
}

#[test]
fn only_local_claude_workers_count_and_exemptions_pass_over_the_limit() {
    let rig = Rig::new();
    rig.status(24.0, 5.0, 1.0);
    denies(&rig, &agent(json!({"description": "d", "prompt": "p"})), "omitted subagent_type (general-purpose) is local and denied");
    denies(&rig, &agent(json!({"subagent_type": "worker-sonnet", "prompt": "p"})), "worker-sonnet is a local Claude worker and denied over the limit");
    allows(&rig, &agent(json!({"subagent_type": "worker", "isolation": "remote", "prompt": "p"})), "remote worker allows over the limit");
    allows(&rig, &agent(json!({"subagent_type": "claude-code-guide", "prompt": "p"})), "non-worker agent type allows");
    allows(&rig, &brief("CASE=URGENT FACT=\"prod login is down for every user\" fix it"), "URGENT brief passes over the limit");
    assert!(rig.file(&rig.dir.path().join("state/verify-overrides.jsonl")).contains(r#""gate": "spawn-capacity""#), "URGENT override is logged");
    allows(&rig, &brief("Category: tooling\nLanding-path: yes\nPick and run checks for each queued landing."), "a Landing-path brief is admitted at the CPU cap");
    allows(&rig, &agent(json!({"subagent_type": "worker", "name": "lander", "prompt": "pick checks per landing"})), "the lander by name is admitted at the CPU cap");
    denies(&rig, &brief("Landing-path: no\nadd a feature"), "an ordinary brief is still denied at the CPU cap");
    allows(&rig, &agent(json!({"subagent_type": "worker-haiku", "prompt": "[routine:watchdog] Run `agents routines show watchdog` and follow it."})), "the capacity watchdog brief is exempt over the limit");
    denies(&rig, &brief("CASE=URGENT FACT=\"short\" go"), "URGENT with a too-short fact is denied");
}

#[test]
fn an_unreadable_probe_allows() {
    let rig = Rig::new();
    allows(&rig, &worker(), "missing probe output allows");
    rig.raw_probe("machine-capacity: shared admission lock remained busy\n");
    allows(&rig, &worker(), "unreadable probe output allows");
    rig.raw_probe(r#"{"decision":"RUN"}"#);
    allows(&rig, &worker(), "probe output without lease fields allows");
}

#[test]
fn codex_spawns_meet_the_same_capacity_rule_and_its_urgent_overrides() {
    let rig = Rig::new();
    rig.status(24.0, 5.0, 1.0);
    denies(&rig, &native("spawn_agent", json!({"message": "fix the bug and land it"}), "/tmp"), "Codex spawn_agent over the limit denies");
    let serialized = json!({"items": [{"type": "text", "text": "CASE=URGENT FACT=\"main red blocks every landing (#242)\" fix it"}]}).to_string();
    allows(&rig, &native("spawn_agent", json!(serialized), "/tmp"), "Codex URGENT inside serialized JSON items allows");
    allows(&rig, &native("spawn_agent", json!({"message": "CASE=URGENT FACT=\"main red blocks every landing (#242)\" fix it"}), "/tmp"), "Codex URGENT in message allows");
    let encrypted = native("collaborationspawn_agent", json!({"message": "gAAAAABqx6PNencrypted", "name": "hellfire_base_293_high", "model": "gpt-6.1-sol", "reasoning_effort": "high"}), "/tmp");
    denies(&rig, &encrypted, "encrypted Codex spawn without a spawn-urgent line is denied");
    let urgent = rig.agents().join("spawn-urgent.tsv");
    rig.write(&urgent, &format!("{}\thellfire_base_293_high\tmain red blocks every landing (#242)\n", rig.now as i64));
    allows(&rig, &encrypted, "encrypted Codex spawn named in a fresh spawn-urgent line allows");
    rig.write(&urgent, &format!("{}\thellfire_base_293_high\tmain red blocks every landing (#242)\n", rig.now as i64 - 1000));
    denies(&rig, &encrypted, "a spawn-urgent line older than 15 minutes is denied");
    rig.status(10.0, 5.0, 1.0);
    allows(&rig, &native("spawn_agent", json!({"message": "fix the bug and land it"}), "/tmp"), "Codex spawn_agent under the limit allows");
}

#[test]
fn a_lower_priority_domain_keeps_headroom_for_the_first() {
    let rig = Rig::new();
    rig.orchestration("[accounts.claude]\nlabels = { \"9b3a7fb5\" = \"a\" }\n[projects.galileo]\npriority = 1\n[projects.makgora]\npriority = 2\n");
    rig.status(17.0, 5.0, 1.0);
    rig.fleet_task("makgora#7");
    let message = denies(&rig, &worker(), "a lower-priority domain stops at 80% of the limit");
    assert!(message.starts_with("Domain makgora ranks below galileo in Tom's priority order, so its local spawns stop at 16 of the 20 CPUs; 17 are committed."), "{message}");
    rig.fleet_task("galileo#9");
    allows(&rig, &worker(), "the first-priority domain keeps the headroom");
}

const TAIL: &str = "Unknown or money-billed usage is never chosen automatically (firn#12). Use another provider's tier from `agents plan`, wait for the window to reset, or report `Blocked: decision for Tom: <question>` for the proxy to put to Tom.";

#[test]
fn the_money_gate_refusal_of_the_session_account_denies() {
    let rig = Rig::new();
    rig.orchestration("[accounts.claude]\nlabels = { \"9b3a7fb5\" = \"a\" }\n");
    rig.status(2.0, 5.0, 1.0);
    let real = |name: &str| format!("No: a new {name} worker could spend money: {name} is at 93% of a plan window (ceiling 92%) and extra_usage would bill money past it. {TAIL}");
    rig.gate(json!(real("claude")), json!({}));
    assert_eq!(denies(&rig, &worker(), "fresh money-gate refusal denies"), real("claude"));
    rig.gate(json!(real("claude")), json!({"claude:a": null}));
    allows(&rig, &worker(), "a refusal written by a process with no account does not refuse a session on account a, which is judged on its own entry");
    rig.gate(Value::Null, json!({"claude:a": real("claude:a")}));
    assert_eq!(denies(&rig, &worker(), "the refusal entry for the session account denies though the shared refusal is null"), real("claude:a"));
}

#[test]
fn an_account_at_or_over_its_pace_line_never_holds_a_spawn() {
    let rig = Rig::new();
    rig.orchestration("[accounts.claude]\nlabels = { \"9b3a7fb5\" = \"a\" }\n[projects.galileo]\npriority = 1\ntarget = 2999-11-01\n[projects.makgora]\npriority = 2\n[pace]\nenforce_from = { \"claude:b\" = \"2000-01-01T00:00:00Z\", \"claude:a\" = \"2999-01-01T00:00:00Z\" }\n");
    rig.status(2.0, 5.0, 1.0);
    let over = json!({"account": "claude:b", "state": "at or over pace", "used_pct": 97.0, "pace_pct": 56.8, "stale": null});
    rig.write(&rig.agents().join("usage-gate.json"), &json!({"claude": {"epoch": rig.now, "stale_min": 30, "refusal": null, "pace": over}}).to_string());
    rig.fleet_task("firn#19");
    allows(&rig, &brief("Item: firn#19\nFix it."), "an enforced at-or-over-pace reading with no target or drain grant never holds a spawn");
}

fn flow_brief(rest: &str) -> Value {
    brief(&format!("Lane: ~/code/makgora/worktrees/fix. {rest}"))
}

#[test]
fn landing_wip_refuses_a_code_worker_past_one_hour_of_throughput() {
    let rig = Rig::new();
    rig.status(2.0, 5.0, 1.0);
    rig.flow_status(3, 1);
    denies(&rig, &flow_brief("Fix the fighter."), "landing WIP refuses makgora above the floor of two");
    allows(&rig, &flow_brief("Fix Autoland."), "landing-path brief passes the landing WIP limit");
    allows(&rig, &flow_brief("CASE=URGENT Fix the fighter."), "CASE=URGENT passes the landing WIP limit");
    allows(&rig, &flow_brief("\nLanding-path: yes\nPick checks per landing."), "a Landing-path brief passes the landing WIP limit");
    allows(&rig, &flow_brief("\nClosing: yes\nRerun Done when on origin/main and tick the boxes."), "a closing pass that pushes no lane passes the landing WIP limit");
    denies(&rig, &flow_brief("Closing: the fighter fix."), "only a Closing: yes line exempts a brief");
    denies(&rig, &brief("Read /home/tom/code/north/main/agent-machinery/skills/verification/SKILL.md. Lane: ~/code/makgora/worktrees/fix. Fix the fighter."), "the lane path, not an earlier north path, names the repository");
    denies(&rig, &native("spawn_agent", json!({"message": "Fix the fighter."}), "/home/test/code/makgora/worktrees/fix"), "Codex cwd uses the same landing WIP limit");
    allows(&rig, &native("spawn_agent", json!({"message": "Read the firn tests and judge ten of them."}), "/home/test/code/makgora/main"), "a spawn from a main checkout with no lane is not a code worker");
    allows(&rig, &brief("Category: research\nLane: ~/code/makgora/worktrees/fix. Survey the tests."), "a research brief is outside the landing WIP limit");
    allows(&rig, &brief("Fix the fighter in ~/code/north/worktrees/fix."), "the brief lane takes precedence over unrelated repositories");
    rig.flow_status(4, 4);
    allows(&rig, &flow_brief("Fix the fighter."), "pending exactly at one hour of landing throughput allows");
    rig.flow_status(5, 4);
    denies(&rig, &flow_brief("Fix the fighter."), "pending above one hour of landing throughput refuses");
    rig.flow_status(2, 1);
    allows(&rig, &flow_brief("Fix the fighter."), "pending at the floor of two allows");
    let denials = rig.file(&rig.home().join(".local/state/agents/spawn-gate-denials.jsonl"));
    let first: Value = serde_json::from_str(denials.lines().next().unwrap()).unwrap();
    assert_eq!((&first["gate"], &first["tool"], &first["input_type"]), (&json!("flow"), &json!("Agent"), &json!("dict")), "{denials}");
    assert!(denials.lines().next().unwrap().starts_with(r#"{"at": ""#), "the denial log keeps its line format: {denials}");
    assert!(denials.lines().any(|line| line.contains("makgora: pending 3, landed 1/h over 3h, push-to-land median 12m") && line.contains("widen the landing path first")), "landing WIP denial logs the repository and all three measurements: {denials}");
}

fn meter(rig: &Rig, five: f64, burn: f64, week: f64, reset: f64) {
    let as_of = Timestamp::from_unix_seconds_f64(rig.now).iso();
    let resets_at = Timestamp::from_unix_seconds_f64(reset).iso();
    let window = |name: &str, used: f64, burn: f64| json!({"window": name, "used_pct": used, "burn_pct_h": burn, "reset_h": 1.0, "as_of": as_of, "age_s": 0.0, "resets_at": resets_at});
    let windows = json!([window("5h", five, burn), window("week", week, 0.0)]);
    let control = crate::ops::pacing::answer(&json!({"op": "advance", "previous": {}, "windows": windows, "workers": 5, "at": rig.now, "initial": 64, "margin": 10, "target": null})).unwrap();
    let verdict = json!({"epoch": rig.now, "stale_min": 30, "pace_control": true, "refusal": null, "refusals": {"claude:a": null}, "controls": {"claude:a": control}});
    rig.write(&rig.agents().join("usage-gate.json"), &json!({"claude": verdict}).to_string());
}

#[test]
fn only_the_one_out_rule_holds_and_names_its_reset() {
    let rig = Rig::new();
    rig.orchestration("[accounts.claude]\nlabels = { \"9b3a7fb5\" = \"a\" }\ntarget_left = 5\n[pace]\nenforce_from = { \"claude:a\" = \"2000-01-01T00:00:00Z\" }\n");
    rig.status(2.0, 5.0, 1.0);
    let reset = rig.now + 3600.0;
    let at = Timestamp::from_unix_seconds_f64(reset).local_twelve_hour();
    let item = agent(json!({"subagent_type": "worker", "model": "opus", "effort": "medium", "prompt": "Item: firn#19"}));
    meter(&rig, 30.0, 5.0, 97.0, reset);
    allows(&rig, &item, "a plan-only account at week 97% with 5 workers in flight and no ledger calibration is admitted");
    meter(&rig, 95.0, 5.0, 60.0, reset);
    assert_eq!(rig.said(&item), Some(format!("Hold: claude:a exhausted: 5h 95% >= 95%; the 5h window resets at {at}.")), "the 5h window at 95% holds and names its reset");
    meter(&rig, 30.0, 50.5, 60.0, reset);
    assert_eq!(rig.said(&item), Some(format!("Hold: claude:a burning: 5h 50.5%/h > the 50%/h cap that keeps the window two hours; the 5h window resets at {at}.")), "5h burn over 50%/h holds and names its reset");
    meter(&rig, 30.0, 5.0, 100.0, reset);
    assert_eq!(rig.said(&item), Some(format!("Hold: claude:a exhausted: provider week 100%; the week window resets at {at}.")), "a provider-reported exhausted week holds and names its reset");
    let money = "No: a new claude:a worker could spend money: claude:a week 93% is past its 92% money ceiling (ceiling 92%) and extra_usage would bill money past it.";
    meter(&rig, 30.0, 5.0, 93.0, reset);
    let mut gate: Value = serde_json::from_str(&rig.file(&rig.agents().join("usage-gate.json"))).unwrap();
    gate["claude"]["refusals"]["claude:a"] = json!(money);
    rig.write(&rig.agents().join("usage-gate.json"), &gate.to_string());
    assert_eq!(rig.said(&item).as_deref(), Some(money), "an account whose overflow bills money past the money ceiling is denied by the usage-gate money refusal");
    assert!(rig.file(&rig.home().join(".local/state/agents/spawn-gate-denials.jsonl")).contains(r#""gate": "projection""#), "an out-rule hold is logged as a projection denial");
}

#[test]
fn a_stale_usage_reading_holds_and_the_width_summary_reads_the_same_gate() {
    let rig = Rig::new();
    rig.status(2.0, 5.0, 1.0);
    rig.write(&rig.agents().join("usage-gate.json"), &json!({"claude": {"epoch": rig.now - 600.0, "stale_min": 30, "refusal": null}}).to_string());
    assert_eq!(rig.said(&worker()).as_deref(), Some("Hold: claude usage remains stale after refresh; needs a reading <=120s old."));
    rig.write(&rig.agents().join("usage-gate.json"), &json!({"claude": {"epoch": rig.now, "stale_min": 30, "refusal": null, "paces": {"claude:a": {}, "claude:b": {}}}}).to_string());
    assert_eq!(gate_summary(&rig), json!({"cpu": true, "accounts": [["claude:a", true], ["claude:b", true]]}));
    rig.status(20.0, 5.0, 1.0);
    assert_eq!(gate_summary(&rig)["cpu"], json!(false));
}

#[test]
fn a_spawn_over_its_money_ceiling_and_the_cpu_limit_names_both_refusals() {
    let rig = Rig::new();
    rig.status(20.0, 5.0, 1.0);
    let money = "No: a new claude worker could spend money: claude is at 93% of a plan window (ceiling 92%) and extra_usage would bill money past it.";
    rig.gate(json!(money), json!({}));
    let said = denies(&rig, &worker(), "a money refusal over the CPU limit denies");
    assert!(said.starts_with(money), "the usage refusal comes first: {said}");
    assert!(said.contains("commit 20 of the 20 CPUs the machine can hand out"), "the capacity refusal is named too: {said}");
}
