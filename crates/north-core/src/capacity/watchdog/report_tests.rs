use serde_json::{Value, json};

use super::report::{Inputs, Run, handoff_fix, parse_time, report};
use super::report_command::capacity_flag;
use crate::ops::provider_capacity::Outcome;

const NOW: &str = "2026-11-05T12:00:00Z";
const HANDOFF_CTX: f64 = 350_000.0;

fn run(agent: &str, tier: &str, role: &str, peak: f64, fires: Option<i64>, ignored: Option<i64>, ended: &str) -> Run {
    Run { agent: agent.to_owned(), tier: tier.to_owned(), role: role.to_owned(), ended: ended.to_owned(), peak_ctx: Some(peak), handoff_fires: fires, handoff_ignored: ignored }
}

fn flags(runs: &[Run], minutes: f64, ledger_since: Option<i64>) -> Vec<String> {
    let inputs = Inputs { samples: &[], incidents: &[], runs, ledger_since, handoff_ctx: HANDOFF_CTX, now: parse_time(NOW).unwrap() };
    report(&inputs, minutes)["flags"].as_array().unwrap().iter().filter_map(|flag| flag.as_str().filter(|flag| flag.starts_with("handoff")).map(str::to_owned)).collect()
}

fn ledger() -> Vec<Run> {
    let recent = "2026-11-05T11:59:00Z";
    vec![
        run("silent", "high", "worker", 364_576.0, Some(0), Some(0), recent),
        run("effective", "high", "worker", 364_576.0, Some(1), Some(0), recent),
        run("marker-only", "high", "worker", 364_576.0, Some(1), None, recent),
        run("ignored", "high", "worker", 20_000.0, Some(1), Some(1), recent),
        run("threshold", "high", "worker", 350_000.0, Some(0), Some(0), recent),
        run("codex", "gpt-6.1-sol high", "worker", 364_576.0, Some(0), Some(0), recent),
        run("gemini", "gemini-3.1-pro high", "worker", 364_576.0, Some(0), Some(0), recent),
        run("parent", "high", "parent", 364_576.0, Some(0), Some(0), recent),
        run("unknown", "high", "worker", 364_576.0, None, Some(0), recent),
        run("older-window", "high", "worker", 364_576.0, Some(0), Some(0), "2026-11-05T11:50:00Z"),
        run("pre-fix", "high", "worker", 364_576.0, Some(0), Some(0), "2026-10-10T10:24:08Z"),
    ]
}

#[test]
fn the_ledger_flags_only_silent_claude_workers_in_the_window_and_since_exposes_the_pre_fix_run() {
    assert_eq!(flags(&ledger(), 5.0, None), ["handoff hook silent: silent peak 365k, 0 fires", "handoff ignored: ignored"]);
    let mut all = flags(&ledger(), 5.0, Some(parse_time("2026-10-10T00:00:00Z").unwrap()));
    all.sort();
    let mut want = vec![
        "handoff hook silent: silent peak 365k, 0 fires",
        "handoff ignored: ignored",
        "handoff hook silent: older-window peak 365k, 0 fires",
        "handoff hook silent: pre-fix peak 365k, 0 fires",
    ];
    want.sort_unstable();
    assert_eq!(all, want);
}

#[test]
fn a_run_before_the_hook_fix_is_outside_even_the_widest_window() {
    assert!(parse_time("2026-10-10T10:24:08Z").unwrap() < handoff_fix());
    assert_eq!(flags(&ledger(), 60.0 * 24.0 * 30.0, None).iter().filter(|flag| flag.contains("pre-fix")).count(), 0);
}

fn sample(at: &str, psi: f64, available: f64) -> Value {
    json!({ "at": at, "load": [1, 1, 1], "cpuPsi": psi, "cpuPsi60": psi, "memPsi": 0, "memFullPsi": 0, "memAvailableMiB": available, "memTotalMiB": 96000, "top": [], "clients": [], "leases": [] })
}

#[test]
fn sustained_cpu_pressure_and_a_falling_memory_floor_raise_flags_and_an_empty_window_says_so() {
    let samples = [sample("2026-11-05T11:58:00Z", 80.0, 30000.0), sample("2026-11-05T11:59:00Z", 90.0, 20000.0), sample("2026-11-05T12:00:00Z", 70.0, 10000.0)];
    let incidents = vec!["- 2026-11-05T11:59:30Z unleased-tree: x".to_owned()];
    let inputs = Inputs { samples: &samples, incidents: &incidents, runs: &[], ledger_since: None, handoff_ctx: HANDOFF_CTX, now: parse_time(NOW).unwrap() };
    let result = report(&inputs, 5.0);
    let found: Vec<&str> = result["flags"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert_eq!(found, ["cpu PSI above 50% in 3/3 samples (max 90)", "memory available 10000 MiB is under the 19200 MiB floor", "1 incident(s) in window"]);
    assert_eq!(result["memAvailableMiB"]["slopePerMin"].as_f64(), Some(-10000.0));
    let empty = report(&Inputs { samples: &[], ..inputs }, 5.0);
    assert_eq!(empty["flags"][0], "no samples: is capacity-watchdog.service running?");
}

#[test]
fn a_recorded_codex_capacity_error_flags_provider_capacity_codex_and_none_flags_nothing() {
    let hit = Outcome { at: "2026-11-05T11:58:00.000Z".to_owned(), model: Some("gpt-6.1-sol".to_owned()), effort: None, kind: "capacity", rollout: String::new() };
    assert_eq!(capacity_flag(Ok(vec![hit])).as_deref(), Some("provider-capacity codex: 1 turn(s) of gpt-6.1-sol at capacity, last 2026-11-05T11:58:00.000Z"));
    assert_eq!(capacity_flag(Ok(Vec::new())), None);
    assert_eq!(capacity_flag(Err("boom".into())).as_deref(), Some("provider-capacity check unavailable: boom"));
}
