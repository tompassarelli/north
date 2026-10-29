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
    claude_runs_on(root, fleet_json, policy, None, &[], "")
}

fn claude_runs_on(root: &Path, fleet_json: &str, policy: &Path, main: Option<&Path>, recorded: &[&str], settled: &str) -> (Vec<Value>, String) {
    let calls = root.join("fleet-calls");
    let fleet = root.join("north");
    fs::write(&fleet, format!("#!/bin/sh\necho \"$@\" >> {}\nprintf '%s' '{fleet_json}'\n", calls.display())).unwrap();
    fs::set_permissions(&fleet, fs::Permissions::from_mode(0o755)).unwrap();
    let empty = root.join("empty");
    fs::write(&empty, settled).unwrap();
    let known = root.join("recorded");
    fs::write(&known, recorded.join("\n")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["ledger", "claude-runs", "--since", "2026-10-08"])
        .arg("--settled")
        .arg(&empty)
        .arg("--recorded")
        .arg(&known)
        .env("HOME", root.join("home"))
        .env("NORTH_BIN", &fleet)
        .env("AGENTS_ORCHESTRATION", policy)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("LEDGER_MAIN")
        .envs(main.map(|main| ("LEDGER_MAIN", main)))
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

#[test]
fn a_haiku_fleet_worker_at_any_effort_is_tier_haiku_and_a_synthetic_last_message_keeps_the_real_model() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-north-main");
    transcript_of(&projects.join("quick.jsonl"), "Item: north#244 labels", "claude-haiku-5-5");
    transcript_of(&projects.join("limited.jsonl"), "Item: north#245 labels", "claude-sonnet-5-5");
    let synthetic = r#"{"type":"assistant","timestamp":"2026-10-08T00:05:00Z","requestId":"r2","message":{"model":"<synthetic>","usage":{"output_tokens":0,"input_tokens":0},"content":[{"type":"text","text":"API Error: limit reached"}]}}"#;
    let mut text = fs::read_to_string(projects.join("limited.jsonl")).unwrap();
    text.push_str(synthetic);
    text.push('\n');
    fs::write(projects.join("limited.jsonl"), text).unwrap();
    let policy = root.join("orchestration.toml");
    fs::write(
        &policy,
        "[tiers.\"claude:haiku\"]\nmodel = \"claude-haiku-5-5\"\neffort = \"low\"\nagent = \"worker-haiku\"\n\
         [tiers.\"claude:sonnet-high\"]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\nagent = \"worker-sonnet\"\n",
    )
    .unwrap();
    let fleet = r#"[{"name":"n-244","conversation":"quick","parent":"lead","task_ref":"north#244","effort":"medium"},
        {"name":"n-245","conversation":"limited","parent":"lead","task_ref":"north#245","effort":"high"}]"#;
    let (rows, _) = claude_runs(root, &fleet.replace('\n', ""), &policy);
    let by_id = |id: &str| rows.iter().find(|row| row["id"] == id).unwrap_or_else(|| panic!("no row for {id}: {rows:?}"));
    assert_eq!((by_id("quick")["tier"].as_str(), by_id("quick")["effort"].as_str()), (Some("haiku"), Some("medium")));
    assert_eq!((by_id("limited")["tier"].as_str(), by_id("limited")["model"].as_str()), (Some("sonnet"), Some("claude-sonnet-5-5")));
}

fn commit_transcript(path: &Path, steps: &[(&str, &str)]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut lines = vec![r#"{"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"s","message":{"content":"Item: north#255 landing"}}"#.to_owned()];
    for (n, (command, output)) in steps.iter().enumerate() {
        let at = format!("2026-10-08T00:0{}:00Z", n + 1);
        lines.push(
            serde_json::json!({"type":"assistant","timestamp":at,"requestId":format!("r{n}"),"message":{"model":"claude-sonnet-5-5","usage":{"output_tokens":5,"input_tokens":5},
                "content":[{"type":"tool_use","id":format!("t{n}"),"name":"Bash","input":{"command":command}}]}})
            .to_string(),
        );
        lines.push(serde_json::json!({"type":"user","timestamp":at,"message":{"content":[{"type":"tool_result","tool_use_id":format!("t{n}"),"content":output}]}}).to_string());
    }
    lines.push(r#"{"type":"assistant","timestamp":"2026-10-08T00:09:00Z","requestId":"rz","message":{"model":"claude-sonnet-5-5","usage":{"output_tokens":5,"input_tokens":5},"content":[{"type":"text","text":"Done: landed."}]}}"#.to_owned());
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

#[test]
fn a_quiet_combined_flag_heredoc_commit_and_a_safe_push_landing_line_both_count_as_landed_on_main() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-north-main");
    let sha = "a".repeat(40);
    commit_transcript(&projects.join("heredoc.jsonl"), &[("git add x && git commit -qF - <<'EOF'\nKeep the real subject (north#255)\nEOF", "")]);
    commit_transcript(&projects.join("pushed.jsonl"), &[("git commit -q --amend -F /tmp/message.txt", ""), ("safe-push --to main > /tmp/push.log; tail -3 /tmp/push.log", &format!("safe-push: landed {sha}\n"))]);
    commit_transcript(&projects.join("reader.jsonl"), &[("rg safe-push src", "src/a.rs"), ("git log --grep safe-push | head", "")]);
    commit_transcript(&projects.join("chained.jsonl"), &[("cd lane && NORTH_X=1 safe-push --to main", "refused\n")]);
    let main = root.join("main");
    fs::write(&main, format!("{} 2026-10-08T01:00:00+00:00 Other work\n{} 2026-10-08T02:00:00+00:00 Keep the real subject (north#255)\n", sha, "b".repeat(40))).unwrap();
    let fleet = r#"[{"name":"h","conversation":"heredoc","parent":"lead"},{"name":"p","conversation":"pushed","parent":"lead"},{"name":"r","conversation":"reader","parent":"lead"},{"name":"c","conversation":"chained","parent":"lead"}]"#;
    let (rows, _) = claude_runs_on(root, fleet, &root.join("absent.toml"), Some(&main), &[], "");
    let by_id = |id: &str| rows.iter().find(|row| row["id"] == id).unwrap_or_else(|| panic!("no row for {id}: {rows:?}"));
    assert_eq!((by_id("heredoc")["landed"].as_i64(), by_id("heredoc")["landed_sha"].as_str()), (Some(1), Some("b".repeat(40).as_str())));
    assert_eq!((by_id("pushed")["landed"].as_i64(), by_id("pushed")["landed_sha"].as_str()), (Some(1), Some(sha.as_str())));
    assert_eq!((by_id("reader")["landed"].as_i64(), by_id("reader")["expects_landing"].as_i64()), (Some(0), Some(0)));
    assert_eq!(by_id("chained")["expects_landing"].as_i64(), Some(1));
}

#[test]
fn a_recorded_worker_now_ending_on_a_notice_is_a_backfill_that_carries_its_landing() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-north-main");
    let sha = "c".repeat(40);
    let path = projects.join("parked.jsonl");
    commit_transcript(&path, &[("git commit -q --amend -F /tmp/message.txt", ""), ("safe-push --to main", &format!("safe-push: landed {sha}\n"))]);
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str(r#"{"type":"user","timestamp":"2026-10-08T00:10:00Z","message":{"content":"[notice] the lane was removed"}}"#);
    text.push('\n');
    fs::write(&path, text).unwrap();
    let main = root.join("main");
    fs::write(&main, format!("{sha} 2026-10-08T02:00:00+00:00 Other work\n")).unwrap();
    let fleet = r#"[{"name":"p","conversation":"parked","parent":"lead"}]"#;
    let (rows, _) = claude_runs_on(root, fleet, &root.join("absent.toml"), Some(&main), &["parked"], "");
    assert_eq!((rows[0]["backfill"].as_bool(), rows[0]["landed"].as_i64(), rows[0]["landed_sha"].as_str()), (Some(true), Some(1), Some(sha.as_str())), "{rows:?}");
}

#[test]
fn a_settled_run_whose_transcript_grew_past_its_recorded_end_is_read_again_and_the_rest_are_not() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-north-main");
    for id in ["resumed", "quiet", "plain"] {
        transcript(&projects.join(format!("{id}.jsonl")), "Item: north#255");
    }
    let fleet = r#"[{"name":"r","conversation":"resumed","parent":"lead"},{"name":"q","conversation":"quiet","parent":"lead"},{"name":"p","conversation":"plain","parent":"lead"}]"#;
    let settled = "resumed\t2026-10-08T00:01:00Z\nquiet\t2026-10-08T00:04:00Z\nplain\n";
    let (rows, _) = claude_runs_on(root, fleet, &root.join("absent.toml"), None, &["resumed", "quiet", "plain"], settled);
    let ids: Vec<&str> = rows.iter().filter_map(|row| row["id"].as_str()).collect();
    assert_eq!(ids, ["resumed"], "{rows:?}");
    assert_eq!((rows[0]["backfill"].as_bool(), rows[0]["ended"].as_str()), (Some(true), Some("2026-10-08T00:04:00Z")));
}

#[test]
fn a_settled_worker_run_that_is_unfinished_again_keeps_its_recorded_end_and_is_not_reread() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path();
    let projects = root.join("home/.claude/projects/-home-tom-code-north-main");
    transcript(&projects.join("stale.jsonl"), "Item: north#290");
    let mut text = fs::read_to_string(projects.join("stale.jsonl")).unwrap();
    text.push_str("{\"type\":\"user\",\"timestamp\":\"2026-10-08T00:05:00Z\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"ok\"}]}}\n");
    fs::write(projects.join("stale.jsonl"), text).unwrap();
    let fleet = r#"[{"name":"s","conversation":"stale","parent":"lead"}]"#;
    let settled = "stale\t2026-10-08T00:01:00Z\n";
    let (rows, _) = claude_runs_on(root, fleet, &root.join("absent.toml"), None, &["stale"], settled);
    assert!(rows.is_empty(), "{rows:?}");
}
