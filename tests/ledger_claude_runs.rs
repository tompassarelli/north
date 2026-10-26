use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn transcript(path: &Path, text: &str) {
    transcript_of(path, text, "claude-opus-5-5");
}

fn transcript_of(path: &Path, text: &str, model: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines = [
        format!(r#"{{"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"s","message":{{"content":"{text}"}}}}"#),
        format!(r#"{{"type":"assistant","timestamp":"2026-10-08T00:04:00Z","requestId":"r1","message":{{"model":"{model}","usage":{{"output_tokens":50,"input_tokens":700}},"content":[{{"type":"text","text":"Done: landed."}}]}}}}"#),
    ];
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn claude_runs(root: &Path, fleet_json: &str, policy: &Path) -> (Vec<Value>, String) {
    let calls = root.join("fleet-calls");
    let fleet = root.join("north");
    fs::write(&fleet, format!("#!/bin/sh\necho \"$@\" >> {}\nprintf '%s' '{fleet_json}'\n", calls.display())).unwrap();
    fs::set_permissions(&fleet, fs::Permissions::from_mode(0o755)).unwrap();
    let empty = root.join("empty");
    fs::write(&empty, "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["ledger", "claude-runs", "--since", "2026-10-08"])
        .arg("--settled")
        .arg(&empty)
        .arg("--recorded")
        .arg(&empty)
        .env("HOME", root.join("home"))
        .env("NORTH_BIN", &fleet)
        .env("AGENTS_ORCHESTRATION", policy)
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let rows = String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    (rows, fs::read_to_string(&calls).unwrap())
}

#[test]
fn claude_runs_lists_the_fleet_once_and_records_only_transcripts_with_a_fleet_role() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom");
    transcript(&projects.join("proxy.jsonl"), "Item: north#1");
    for n in 0..5 {
        transcript(&projects.join(format!("probe-{n}.jsonl")), "probe");
    }
    let (rows, calls) = claude_runs(root, r#"[{"name":"p","conversation":"proxy","parent":null}]"#, &root.join("absent.toml"));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!((rows[0]["id"].as_str(), rows[0]["role"].as_str(), rows[0]["outcome"].as_str()), (Some("proxy"), Some("proxy"), Some("done")));
    assert_eq!(calls.lines().count(), 1);
}

#[test]
fn a_fleet_or_worker_subagent_run_is_a_worker_run_on_its_model_and_efforts_tier_whether_dispatched_or_forgotten() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-smashcraft-main");
    transcript_of(&projects.join("dispatched.jsonl"), "Item: smashcraft#459 back-air", "claude-opus-5-5");
    transcript_of(&projects.join("child.jsonl"), "Item: north#240 ledger", "claude-sonnet-5-5");
    transcript_of(&projects.join("deep.jsonl"), "Item: north#241 plan", "claude-opus-5-5");
    transcript(&projects.join("proxy.jsonl"), "hello");
    for (agent, kind) in [("aworker", "worker-sonnet"), ("aexplore", "Explore")] {
        transcript_of(&projects.join(format!("proxy/subagents/agent-{agent}.jsonl")), "Item: north#242 sub", "claude-sonnet-5-5");
        fs::write(projects.join(format!("proxy/subagents/agent-{agent}.meta.json")), format!(r#"{{"agentType":"{kind}"}}"#)).unwrap();
    }
    let policy = root.join("orchestration.toml");
    fs::write(
        &policy,
        "[tiers.\"claude:sonnet-high\"]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\nagent = \"worker-sonnet\"\n\
         [tiers.\"claude:opus-medium\"]\nmodel = \"claude-opus-5-5\"\neffort = \"medium\"\nagent = \"worker\"\n",
    )
    .unwrap();
    let fleet = r#"[{"name":"proxy","conversation":"proxy","parent":null,"task_ref":null,"effort":null},
        {"name":"sc-459","conversation":"dispatched","parent":null,"task_ref":"smashcraft#459","effort":null,"past":true},
        {"name":"n-240","conversation":"child","parent":"lead","task_ref":null,"effort":"high","past":true},
        {"name":"n-241","conversation":"deep","parent":"lead","task_ref":"north#241","effort":"max"}]"#;
    let (rows, calls) = claude_runs(root, &fleet.replace('\n', ""), &policy);
    assert_eq!(calls.trim(), "fleet list --json --all", "the ledger reads forgotten and placed sessions too");
    let by_id = |id: &str| rows.iter().find(|row| row["id"] == id).unwrap_or_else(|| panic!("no row for {id}: {rows:?}"));
    assert!(rows.iter().all(|row| row["id"] != "aexplore"), "a subagent that is not a worker is no run");
    for (id, tier, effort) in [("dispatched", "medium", "medium"), ("child", "sonnet", "high"), ("deep", "claude-opus-5-5 max", "max"), ("aworker", "sonnet", "high")] {
        let row = by_id(id);
        assert_eq!((row["role"].as_str(), row["tier"].as_str(), row["effort"].as_str(), row["outcome"].as_str()), (Some("worker"), Some(tier), Some(effort), Some("done")), "{id}");
    }
    assert!(by_id("dispatched")["brief"].as_str().unwrap().starts_with("Item: smashcraft#459"));
    assert_eq!((by_id("proxy")["role"].as_str(), by_id("proxy")["tier"].as_str()), (Some("proxy"), Some("claude-opus-5-5")));
}
