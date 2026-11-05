use std::fs;

use serde_json::json;

use super::*;

const HOUR: u64 = 3_600_000;
const UNITS: &str = "# /home/tom/.config/systemd/user/capacity-watchdog.service -> /nix/store/d1s7-capacity-watchdog.service/capacity-watchdog.service\n[Service]\nEnvironment=PATH=/run/wrappers/bin\nExecStart=/home/tom/.local/share/north/bin/capacity-watchdog sample\nExecStartPre=-/bin/true\n# /home/tom/.config/systemd/user/agents-flow.service\n[Service]\nExecStart=/usr/bin/env bash /home/tom/code/south/main/bin/agents-flow --once\n";

fn host_with_hooks(hooks: &[&str]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let directory = tempfile::tempdir().expect("fixture");
    let home = directory.path().join("home");
    let state = home.join(".local/state");
    let settings = state.join("north/profiles/claude-a/settings.json");
    fs::create_dir_all(settings.parent().expect("parent")).expect("profile");
    let commands: Vec<_> = hooks
        .iter()
        .map(|hook| json!({"type": "command", "command": format!("/bin/env PATH=/x /bin/bash {}/.agents/hooks/{hook}", home.display())}))
        .collect();
    fs::write(&settings, json!({"hooks": {"PreToolUse": [{"hooks": commands}]}}).to_string()).expect("settings");
    (directory, home, state)
}

#[test]
fn a_host_registering_prompt_grounding_blocks_its_removal_and_one_that_stopped_does_not() {
    let (_directory, home, state) = host_with_hooks(&["prompt-grounding.sh", "kept.sh"]);
    let registering = collect("whiterabbit", 1_000, &home, &state, "");
    let (_other, other_home, other_state) = host_with_hooks(&["kept.sh"]);
    let stopped = collect("gex45", 1_000, &other_home, &other_state, "");
    let report = judge(&[registering, stopped], "prompt-grounding.sh", &[], 2_000, HOUR);
    assert_eq!(report.naming.len(), 1, "{report:?}");
    assert!(report.naming[0].starts_with("whiterabbit: ") && report.naming[0].contains("prompt-grounding.sh registered in "), "{report:?}");
    assert!(report.unknown.is_empty());
    let (_third, third_home, third_state) = host_with_hooks(&["kept.sh"]);
    let clean = judge(&[collect("whiterabbit", 1_000, &third_home, &third_state, "")], "prompt-grounding.sh", &[], 2_000, HOUR);
    assert!(clean.is_clean());
}

#[test]
fn a_unit_whose_exec_start_runs_capacity_watchdog_blocks_deleting_it_by_name_or_bin_path() {
    let facts = Facts { schema: SCHEMA.into(), host: "gex45".into(), recorded_at_ms: 1_000, hooks: Vec::new(), unit_exec_starts: unit_exec_starts(UNITS) };
    assert_eq!(facts.unit_exec_starts.len(), 4, "{:?}", facts.unit_exec_starts);
    for name in ["capacity-watchdog", "bin/capacity-watchdog"] {
        let report = judge(std::slice::from_ref(&facts), name, &[], 2_000, HOUR);
        assert_eq!(report.naming.len(), 1, "{name}: {report:?}");
        assert!(report.naming[0].contains("capacity-watchdog.service runs /home/tom/.local/share/north/bin/capacity-watchdog"), "{report:?}");
    }
    assert_eq!(judge(std::slice::from_ref(&facts), "agents-flow", &[], 2_000, HOUR).naming.len(), 1);
    assert!(judge(std::slice::from_ref(&facts), "watchdog", &[], 2_000, HOUR).is_clean());
}

#[test]
fn facts_older_than_the_max_age_and_hosts_with_no_facts_are_unknown_and_refuse() {
    let (_directory, home, state) = host_with_hooks(&["kept.sh"]);
    let old = collect("nexus", 1_000, &home, &state, "");
    let report = judge(std::slice::from_ref(&old), "gone.sh", &["nexus".into(), "cx53".into()], 1_000 + 25 * HOUR, 24 * HOUR);
    assert!(!report.is_clean());
    assert!(report.naming.is_empty());
    assert_eq!(report.unknown.len(), 2, "{report:?}");
    assert!(judge(std::slice::from_ref(&old), "gone.sh", &["nexus".into()], 1_000 + HOUR, 24 * HOUR).is_clean());
}

#[test]
fn recorded_facts_round_trip_through_the_facts_directory() {
    let (directory, home, state) = host_with_hooks(&["prompt-grounding.sh"]);
    let facts = collect("nexus", 5_000, &home, &state, UNITS);
    let facts_dir = directory.path().join("host-facts");
    let written = write(&facts_dir, &facts).expect("write");
    assert_eq!(written, facts_dir.join("nexus.json"));
    assert_eq!(read_all(&facts_dir).expect("read"), vec![facts]);
    let report = hosts_naming(&facts_dir, "prompt-grounding.sh", &[], 6_000, HOUR).expect("report");
    assert_eq!(report.naming.len(), 1);
    assert!(read_all(&directory.path().join("absent")).expect("absent dir").is_empty());
}

#[test]
fn durations_parse_with_unit_suffixes() {
    assert_eq!(parse_duration_ms("24h").expect("hours"), 24 * HOUR);
    assert_eq!(parse_duration_ms("90m").expect("minutes"), 90 * 60_000);
    assert_eq!(parse_duration_ms("30").expect("seconds"), 30_000);
    assert!(parse_duration_ms("soon").is_err());
}

#[test]
fn a_facts_directory_with_no_host_in_it_refuses_instead_of_passing_silently() {
    let report = judge(&[], "prompt-grounding.sh", &[], 2_000, HOUR);
    assert_eq!(report.unknown, ["no host has recorded facts"]);
    assert!(!report.is_clean());
}

#[test]
fn the_report_lists_each_recorded_host_with_its_fact_age() {
    let (_directory, home, state) = host_with_hooks(&["kept.sh"]);
    let report = judge(&[collect("gex45", 1_000, &home, &state, "")], "gone.sh", &[], 8_000, HOUR);
    assert_eq!(report.ages, ["gex45 7 s"]);
}
