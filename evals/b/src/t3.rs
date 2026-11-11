use std::fs;
use std::path::Path;

use serde_json::{Value, json};

pub const PUBLIC_EXPECT: &str = "/tmp/eval-b-public/expect";

pub fn tasks(workload: &Path, count: usize, out: &Path) -> Result<(), String> {
    let manifest: Value = serde_json::from_str(&fs::read_to_string(workload.join("tasks.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::create_dir_all(PUBLIC_EXPECT).map_err(|e| e.to_string())?;
    let repo = workload.join("repo");
    let tasks: Vec<Value> = manifest["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .take(count)
        .map(|task| {
            let id = task["id"].as_str().unwrap_or_default();
            let _ = fs::copy(workload.join("expect").join(format!("{id}.test.ts")), Path::new(PUBLIC_EXPECT).join(format!("{id}.test.ts")));
            json!({"id": id, "repo": "eval-b-workload", "repo_path": repo, "base": manifest["base"], "reference": [], "text": task["text"],
                   "check": task["check"], "check_timeout_s": 120, "hidden_check": format!("cmp -s test/{id}.test.ts {PUBLIC_EXPECT}/{id}.test.ts && bun test"),
                   "setup": "true", "kind": "small", "pilot": true, "verified": true})
        })
        .collect();
    fs::write(out, json!({"smoke": false, "task": tasks}).to_string()).map_err(|e| e.to_string())
}

pub fn rows(from: &Path, shared: &Value) -> Result<Vec<Value>, String> {
    let text = fs::read_to_string(from).map_err(|e| e.to_string())?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .map(|a| {
            let landed = a["abandoned"] == false;
            let id = a["task"].as_str().unwrap_or_default();
            let tokens = &a["tokens"];
            let total: u64 = ["input", "output", "cache_read", "cache_write"].iter().filter_map(|k| tokens[k].as_u64()).sum();
            json!({
                "arm": "t3-sequential", "level": 1, "task": id, "shared": shared[id], "launched_at": a["started"],
                "launch_epoch": a["started"].as_str().and_then(local_epoch),
                "landed": landed, "merged": a["merged"], "t_land_s": if landed { a["wall_s"].clone() } else { Value::Null },
                "failed": !landed, "failure": a["abandon_reason"], "acceptance": a["acceptance"], "hidden_check": a["hidden_check"],
                "handins": 1 + a["failed_handins"].as_u64().unwrap_or(0), "failed_handins": a["failed_handins"], "redos": 0, "reworks": 0, "rework": false, "conflict": false, "conflict_refusals": 0,
                "queue_wait_s": 0, "interventions": a["interventions"], "intervention_stalls": a["intervention_stalls"],
                "tokens": tokens, "tokens_total_spend": total, "cost_usd": a["cost_usd"],
                "conditions": a["conditions"], "command": a["command"], "raw": a["raw"],
            })
        })
        .collect())
}

fn local_epoch(text: &str) -> Option<f64> {
    let out = crate::proc::run("date", &["-d", text, "+%s"], None);
    out.out.trim().parse().ok()
}
