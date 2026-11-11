use std::path::Path;

use serde_json::{Value, json};

use crate::collect;
use crate::ladder::{EFFORT, MODEL, Plan, Run};
use crate::proc::{git, local_time, run, sh, ssh};
use crate::scratch;

pub fn conditions(plan: &Plan, base: &str, seed: u64) -> Value {
    let harness: serde_json::Map<String, Value> = plan.hosts.iter().map(|h| (h.clone(), json!(ssh(h, "cut -c1-12 ~/.local/state/north/current/built-from").out.trim()))).collect();
    let listed: Value = serde_json::from_str(&run("north", &["fleet", "list", "--json"], None).out).unwrap_or(Value::Null);
    let working = listed.as_array().map(|a| a.iter().filter(|s| s["status"] == "working" || s["status"] == "starting").count());
    let loads: serde_json::Map<String, Value> = plan.hosts.iter().map(|h| (h.clone(), json!(ssh(h, "cut -d' ' -f1-3 /proc/loadavg").out.trim()))).collect();
    json!({
        "level": plan.level, "tasks_in_level": plan.tasks.len(),
        "shared_file_share": plan.tasks.iter().filter(|t| t["shared"] == true).count() as f64 / plan.tasks.len().max(1) as f64,
        "orchestrator": sh("hostname -s").out.trim(), "laptop_load_at_start": sh("cut -d' ' -f1-3 /proc/loadavg").out.trim(),
        "host_load_at_start": loads, "laptop_north_built_from": sh("cut -c1-12 ~/.local/state/north/current/built-from").out.trim(), "other_fleet_sessions_working": working,
        "model": MODEL, "effort_requested": EFFORT, "account": plan.account, "harness_north_built_from": harness,
        "repo": format!("eval-b synthetic workload, seed {seed}"), "base": base,
        "caches": "cold lanes: fresh worktrees on every host; the workload has no dependencies to warm",
        "local_time_at_start": local_time(crate::proc::now()),
    })
}

pub fn verify(work: &Path) -> Result<(), String> {
    let verify = work.join("verify");
    let _ = sh(&format!("rm -rf '{}'", verify.display()));
    crate::proc::must(git(work, &["clone", "-q", &scratch::origin().to_string_lossy(), "verify"]), "clone landed main")?;
    let green = sh(&format!("cd '{}' && PATH={}:$PATH bun test >/dev/null 2>&1", verify.display(), crate::proc::TOOLPATH)).ok();
    let _ = std::fs::write(work.join("main-green"), if green { "pass" } else { "fail" });
    Ok(())
}

fn accepted(work: &Path, expect: &Path, task: &Value) -> (bool, &'static str) {
    let verify = work.join("verify");
    let id = task["id"].as_str().unwrap_or_default();
    let check = task["check"].as_str().unwrap_or_default();
    let acceptance = sh(&format!("cd '{}' && PATH={}:$PATH {check} >/dev/null 2>&1", verify.display(), crate::proc::TOOLPATH)).ok();
    let hidden = sh(&format!("cmp -s '{}/test/{id}.test.ts' '{}/{id}.test.ts'", verify.display(), expect.display())).ok();
    (acceptance, if hidden { "pass" } else { "fail" })
}

fn after(launch: f64, at: Option<f64>) -> Value {
    at.map_or(Value::Null, |t| json!((t - launch).round()))
}

pub fn row(plan: &Plan, run: &Run, expect: &Path, stamp: &Value, command: &str) -> Value {
    let dir = plan.work.join(&run.name);
    let session: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap_or_default()).unwrap_or(Value::Null);
    let transcript = collect::transcript(&dir);
    let attempts = collect::spans(scratch::queue_dir().as_deref(), &run.branch);
    let (cost, total) = collect::spend(&dir);
    let (resolved_at, reason) = run.resolved.clone().unwrap_or((run.launch_end, "unresolved".to_owned()));
    let landed = reason == "landed";
    let (acceptance, hidden) = if landed { accepted(&plan.work, expect, &run.task) } else { (false, "not-run") };
    let landed_attempt = attempts.as_array().and_then(|a| a.iter().find(|x| x["attributes"]["landed"].is_string())).cloned();
    let first_queued = attempts.as_array().and_then(|a| a.iter().filter_map(|x| x["queued_at"].as_f64()).reduce(f64::min));
    let handins = transcript["handins"].as_u64().unwrap_or(0);
    let lander_refusals = attempts.as_array().map_or(0, |a| a.iter().filter(|x| !x["attributes"]["landed"].is_string()).count());
    let conflicts = transcript["conflict_mentions"].as_u64().unwrap_or(0);
    let mut conditions = stamp.clone();
    conditions["host"] = session["machine"].clone();
    conditions["claude_code_observed"] = transcript["claude_code"].clone();
    conditions["models_observed"] = transcript["models"].clone();
    conditions["effort_observed"] = transcript["efforts"].clone();
    json!({
        "arm": "north", "level": plan.level, "task": run.task["id"], "shared": run.task["shared"], "session": run.name, "branch": run.branch,
        "launched_at": local_time(run.launch_start), "launch_epoch": run.launch_start, "launch_s": (run.launch_end - run.launch_start).round(), "launch_exit": run.launch_exit,
        "launch_output": run.launch_output.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect::<String>(),
        "first_request_s": after(run.launch_start, transcript["first_request_at"].as_f64()),
        "handin_s": after(run.launch_start, first_queued),
        "queue_wait_s": landed_attempt.as_ref().map_or(Value::Null, |a| a["queue_wait_s"].clone()),
        "landed": landed && acceptance, "merged": landed, "t_land_s": if landed { after(run.launch_start, Some(resolved_at)) } else { Value::Null },
        "failed": !(landed && acceptance), "failure": if landed && !acceptance { "acceptance-failed".to_owned() } else if landed { String::new() } else { reason },
        "acceptance": acceptance, "hidden_check": hidden,
        "handins": handins, "redos": handins.saturating_sub(1), "lander_attempts": attempts.as_array().map_or(0, Vec::len), "lander_refusals": lander_refusals,
        "conflict": conflicts > 0 || lander_refusals > 0, "conflict_mentions": conflicts,
        "interventions": run.nudges.len(), "intervention_stalls": run.nudges,
        "tokens": transcript["tokens"], "tokens_total_spend": total, "cost_usd": cost,
        "last_words": if landed { String::new() } else { collect::last_words(&dir) },
        "conditions": conditions, "command": command, "raw": dir,
    })
}
