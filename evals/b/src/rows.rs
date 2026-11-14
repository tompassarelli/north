use std::path::Path;

use serde_json::{Value, json};

use crate::collect;
use crate::ladder::{Plan, Run};
use crate::proc::{git, local_time, run, sh, ssh};
use crate::scratch;

pub fn conditions(plan: &Plan, base: &str, seed: u64) -> Value {
    let harness: serde_json::Map<String, Value> = plan.hosts.iter().map(|h| (h.clone(), json!(ssh(h, "cut -c1-12 ~/.local/state/north/current/built-from").out.trim()))).collect();
    let listed: Value = serde_json::from_str(&run("north", &["fleet", "list", "--json"], None).out).unwrap_or(Value::Null);
    let working = listed.as_array().map(|a| a.iter().filter(|s| s["status"] == "working" || s["status"] == "starting").count());
    json!({
        "level": plan.level, "tasks_in_level": plan.tasks.len(),
        "shared_file_share": plan.tasks.iter().filter(|t| t["shared"] == true).count() as f64 / plan.tasks.len().max(1) as f64,
        "orchestrator": sh("hostname -s").out.trim(), "laptop_load_at_start": sh("cut -d' ' -f1-3 /proc/loadavg").out.trim(),
        "host_load_at_start": crate::proc::host_loads(&plan.hosts), "laptop_north_built_from": sh("cut -c1-12 ~/.local/state/north/current/built-from").out.trim(), "other_fleet_sessions_working": working,
        "model": plan.model, "effort_requested": plan.effort, "context_arm": "context-500k, pinned in every brief so the spare fill before the first wave and the launches draw the same compaction window", "account": plan.accounts.join(","), "harness_north_built_from": harness,
        "repo": format!("eval-b synthetic workload, seed {seed}"), "base": base,
        "caches": "cold lanes: fresh worktrees on every host; the workload has no dependencies to warm",
        "local_time_at_start": local_time(crate::proc::now()),
        "landed_checks_on": plan.checks_on.as_deref().map_or_else(|| "laptop".to_owned(), |host| format!("{host}, in depbox run")),
    })
}

pub fn verify(work: &Path, checks_on: Option<&str>) -> Result<(), String> {
    let verify = work.join("verify");
    let _ = sh(&format!("rm -rf '{}'", verify.display()));
    crate::proc::must(git(work, &["clone", "-q", &scratch::origin().to_string_lossy(), "verify"]), "clone landed main")?;
    if let Some(host) = checks_on {
        let remote = scratch::host_verify();
        crate::proc::must(ssh(host, &format!("rm -rf {remote} {remote}.git && git init -q --bare {remote}.git")), &format!("make {host}'s verify origin"))?;
        crate::proc::must(git(&verify, &["push", "-q", &format!("ssh://tom@{host}{remote}.git"), "HEAD:refs/heads/main"]), &format!("push landed main to {host}"))?;
        crate::proc::must(ssh(host, &format!("git clone -q {remote}.git {remote}")), &format!("clone landed main on {host}"))?;
    }
    let green = landed_check(work, checks_on, "bun test");
    let _ = std::fs::write(work.join("main-green"), if green { "pass" } else { "fail" });
    Ok(())
}

fn landed_check(work: &Path, checks_on: Option<&str>, command: &str) -> bool {
    let toolpath = crate::proc::TOOLPATH;
    match checks_on {
        None => sh(&format!("cd '{}' && PATH={toolpath}:$PATH {command} >/dev/null 2>&1", work.join("verify").display())).ok(),
        Some(host) => ssh(host, &format!("cd {} && PATH={toolpath}:$PATH depbox run -- bash -c '{}' >/dev/null 2>&1", scratch::host_verify(), command.replace('\'', "'\\''"))).ok(),
    }
}

fn accepted(work: &Path, checks_on: Option<&str>, expect: &Path, task: &Value) -> (bool, &'static str) {
    let verify = work.join("verify");
    let id = task["id"].as_str().unwrap_or_default();
    let acceptance = landed_check(work, checks_on, task["check"].as_str().unwrap_or_default());
    let hidden = sh(&format!("cmp -s '{}/test/{id}.test.ts' '{}/{id}.test.ts'", verify.display(), expect.display())).ok();
    (acceptance, if hidden { "pass" } else { "fail" })
}

fn main_commits(work: &Path, task: &Value) -> usize {
    let id = task["id"].as_str().unwrap_or_default();
    git(&work.join("verify"), &["log", "--format=%H", "--", &format!("src/{id}.ts")]).out.lines().count().saturating_sub(1)
}

fn after(launch: f64, at: Option<f64>) -> Value {
    at.map_or(Value::Null, |t| json!((t - launch).round()))
}

pub fn row(plan: &Plan, run: &Run, expect: &Path, stamp: &Value, command: &str) -> Value {
    let dir = plan.work.join(&run.name);
    let session: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("session.json")).unwrap_or_default()).unwrap_or(Value::Null);
    let transcript = collect::transcript(&dir);
    let attempts = collect::spans(scratch::queue_dir().as_deref(), &run.branch, run.launch_start);
    let (cost, total) = collect::spend(&dir);
    let (resolved_at, reason) = run.resolved.clone().unwrap_or((run.launch_end, "unresolved".to_owned()));
    let landed = reason == "landed";
    let (acceptance, hidden) = if landed { accepted(&plan.work, plan.checks_on.as_deref(), expect, &run.task) } else { (false, "not-run") };
    let landed_attempt = attempts.as_array().and_then(|a| a.iter().find(|x| x["attributes"]["landed"].is_string())).cloned();
    let first_queued = attempts.as_array().and_then(|a| a.iter().filter_map(|x| x["queued_at"].as_f64()).reduce(f64::min));
    let lander_refusals = attempts.as_array().map_or(0, |a| a.iter().filter(|x| !x["attributes"]["landed"].is_string()).count());
    let mut conditions = stamp.clone();
    conditions["account"] = json!(run.account);
    conditions["host"] = json!(collect::machine(&session));
    conditions["host_load_at_launch"] = run.load_at_launch.clone();
    conditions["admission_queued_at_launch"] = json!(run.queued_at_launch);
    conditions["admission_wait_s"] = json!((run.room_wait_s * 100.0).round() / 100.0);
    conditions["host_load_at_end"] = serde_json::from_str(&std::fs::read_to_string(dir.join("load-at-end.json")).unwrap_or_default()).unwrap_or(Value::Null);
    conditions["claude_code_observed"] = transcript["claude_code"].clone();
    conditions["models_observed"] = transcript["models"].clone();
    conditions["effort_observed"] = transcript["efforts"].clone();
    let mut row = json!({
        "arm": "north", "level": plan.level, "task": run.task["id"], "shared": run.task["shared"], "session": run.name, "branch": run.branch,
        "launched_at": local_time(run.launch_start), "launch_epoch": run.launch_start, "launch_s": (run.launch_end - run.launch_start).round(), "launch_exit": run.launch_exit,
        "launch_output": run.launch_output.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect::<String>(),
        "first_request_s": after(run.launch_start, transcript["first_request_at"].as_f64()),
        "handin_s": after(run.launch_start, first_queued),
        "verdict_s": landed_attempt.as_ref().map_or(Value::Null, |a| a["verdict_s"].clone()),
        "queue_wait_s": landed_attempt.as_ref().map_or(Value::Null, |a| a["queue_wait_s"].clone()),
        "landed": landed && acceptance, "merged": landed, "t_land_s": if landed { after(run.launch_start, Some(resolved_at)) } else { Value::Null },
        "failed": !(landed && acceptance), "failure": if landed && !acceptance { "acceptance-failed".to_owned() } else if landed { String::new() } else { reason },
        "acceptance": acceptance, "hidden_check": hidden,
        "lander_attempts": attempts.as_array().map_or(0, Vec::len), "lander_refusals": lander_refusals,
        "landings": attempts.as_array().map_or(0, |a| a.iter().filter(|x| x["attributes"]["landed"].is_string()).count()), "main_commits": main_commits(&plan.work, &run.task), "wave": run.wave,
        "interventions": run.nudges.len(), "intervention_stalls": run.nudges,
        "model_requests": transcript["model_requests"], "tokens": transcript["tokens"], "tokens_total_spend": total, "cost_usd": cost,
        "last_words": if landed { String::new() } else { collect::last_words(&dir) },
        "conditions": conditions, "command": command, "raw": dir,
    });
    counted(&mut row, &transcript["handins"]);
    row
}

pub fn counted(row: &mut Value, handins: &Value) {
    let before = handins["conflict_refusals_before_queue"].as_u64().unwrap_or(0);
    let by_lander = handins["conflict_refusals_by_lander"].as_u64().unwrap_or(0);
    let reworks = handins["reworks"].as_u64().unwrap_or(0);
    for key in ["handins", "handins_by_tool", "conflict_refusals_before_queue", "conflict_refusals_by_lander", "hook_blocks", "fetch_lock_failures", "reworks"] {
        row[key] = handins[key].clone();
    }
    row["conflict_refusals"] = json!(before + by_lander);
    row["conflict"] = json!(before + by_lander > 0);
    row["rework"] = json!(reworks > 0);
    row["redos"] = json!(reworks);
    if let Some(map) = row.as_object_mut() {
        map.remove("conflict_mentions");
    }
}
