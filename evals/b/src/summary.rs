use std::collections::BTreeMap;

use serde_json::Value;

use crate::proc::percentile;

fn number(row: &Value, key: &str) -> Option<f64> {
    row[key].as_f64()
}

fn seconds(value: Option<f64>) -> String {
    value.map_or("-".to_owned(), |v| format!("{v:.0} s"))
}

fn share(part: usize, whole: usize) -> String {
    format!("{part} of {whole} ({:.0}%)", 100.0 * part as f64 / whole.max(1) as f64)
}

fn level_block(arm: &str, level: u64, rows: &[&Value]) -> Vec<String> {
    let landed: Vec<&&Value> = rows.iter().filter(|r| r["landed"] == true).collect();
    let land: Vec<f64> = landed.iter().filter_map(|r| number(r, "t_land_s")).collect();
    let waits: Vec<f64> = landed.iter().filter_map(|r| number(r, "queue_wait_s")).collect();
    let start = rows.iter().filter_map(|r| number(r, "launch_epoch")).reduce(f64::min);
    let end = landed.iter().filter_map(|r| Some(number(r, "launch_epoch")? + number(r, "t_land_s")?)).reduce(f64::max);
    let per_hour = match (start, end) {
        (Some(s), Some(e)) if e > s => format!("{:.1}", landed.len() as f64 * 3600.0 / (e - s)),
        _ => "-".to_owned(),
    };
    let tokens: u64 = rows.iter().filter_map(|r| r["tokens_total_spend"].as_u64()).sum();
    let cost: f64 = rows.iter().filter_map(|r| r["cost_usd"].as_f64()).sum();
    let failures: BTreeMap<String, usize> = rows.iter().filter(|r| r["failed"] == true).fold(BTreeMap::new(), |mut m, r| {
        *m.entry(r["failure"].as_str().unwrap_or("unknown").to_owned()).or_default() += 1;
        m
    });
    let hosts: BTreeMap<String, usize> = rows.iter().fold(BTreeMap::new(), |mut m, r| {
        *m.entry(r["conditions"]["host"].as_str().or(r["conditions"]["machine"].as_str()).unwrap_or("none").to_owned()).or_default() += 1;
        m
    });
    let stamp = &rows[0]["conditions"];
    vec![
        format!("- {arm}, level {level}: {} tasks", rows.len()),
        format!("  - landed changes per hour: {per_hour} ({} landed, first launch to last landing)", landed.len()),
        format!("  - seconds to land, landed tasks: p50 {}, p95 {}", seconds(percentile(&land, 0.5)), seconds(percentile(&land, 0.95))),
        format!("  - queue wait: p50 {}, p95 {}", seconds(percentile(&waits, 0.5)), seconds(percentile(&waits, 0.95))),
        format!("  - conflict rate: {}; rework rate (more than one hand-in): {}", share(rows.iter().filter(|r| r["conflict"] == true).count(), rows.len()), share(rows.iter().filter(|r| r["redos"].as_u64().unwrap_or(0) > 0).count(), rows.len())),
        format!("  - worker failure rate: {} {failures:?}", share(rows.iter().filter(|r| r["failed"] == true).count(), rows.len())),
        format!("  - interventions (scripted nudges): {}", rows.iter().filter_map(|r| r["interventions"].as_u64()).sum::<u64>()),
        format!("  - tokens: {tokens} in all, {} per task; API-equivalent ${cost:.2}", tokens / rows.len().max(1) as u64),
        format!("  - conditions: placed {hosts:?}; model {} at {}; account {}; harness {}; laptop load {}; other fleet sessions working {}; {}; {}",
            stamp["model"], stamp["effort_requested"], stamp["account"], stamp.get("harness_north_built_from").or(stamp.get("harness")).unwrap_or(&Value::Null),
            stamp.get("laptop_load_at_start").or(stamp.get("load_at_start")).unwrap_or(&Value::Null), stamp["other_fleet_sessions_working"], stamp["caches"], stamp.get("local_time_at_start").unwrap_or(&rows[0]["launched_at"])),
    ]
}

fn sequential_scale(rows: &[&Value]) -> Vec<String> {
    let walls: Vec<f64> = rows.iter().filter_map(|r| number(r, "t_land_s")).collect();
    if walls.is_empty() {
        return Vec::new();
    }
    let mean = walls.iter().sum::<f64>() / walls.len() as f64;
    vec![format!(
        "  - scaled by arithmetic, not run: one at a time at the mean of {mean:.0} s a task this arm lands {:.1} changes an hour, and 5 tasks take {:.0} s and 21 take {:.0} s end to end",
        3600.0 / mean, 5.0 * mean, 21.0 * mean
    )]
}

pub fn print(rows: &[Value]) {
    let mut groups: BTreeMap<(String, u64), Vec<&Value>> = BTreeMap::new();
    for row in rows {
        groups.entry((row["arm"].as_str().unwrap_or_default().to_owned(), row["level"].as_u64().unwrap_or(0))).or_default().push(row);
    }
    println!("Eval B result");
    for ((arm, level), group) in &groups {
        for line in level_block(arm, *level, group) {
            println!("{line}");
        }
        if arm != "north" {
            for line in sequential_scale(group) {
                println!("{line}");
            }
        }
    }
}
