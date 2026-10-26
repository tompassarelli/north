use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde_json::json;

use super::Inputs;
use super::report::report;
use crate::foundation::clock::Timestamp;

const AGENT_JOURNAL: &str = "2026-10-10T10:00:01+00:00 nexus kernel: Out of memory: Killed process 4242 (claude) total-vm:9000000kB\n\
2026-09-01T10:00:01+00:00 nexus kernel: Out of memory: Killed process 1 (claude) total-vm:1kB\n";
const OTHER_JOURNAL: &str = "2026-10-10T10:00:01+00:00 nexus kernel: Out of memory: Killed process 4243 (postgres) total-vm:9000000kB\n\
2026-10-11T10:00:01+00:00 nexus earlyoom[519]: sending SIGTERM to process 77 uid 1000 \"node\": oom_score 300, VmRSS 900 MB, cmdline \"node fixture-app\"\n";

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Psi,
    Mem,
}

struct Week<'a> {
    episode_days: &'a [i64],
    minutes: i64,
    kind: Kind,
    steal_minutes: i64,
    missing: std::ops::Range<i64>,
}

const QUIET: Week = Week { episode_days: &[], minutes: 0, kind: Kind::Psi, steal_minutes: 0, missing: 0..0 };

fn write_week(dir: &Path, week: &Week) {
    fs::create_dir_all(dir).unwrap();
    let start = Timestamp::parse("2026-10-07T00:00:00Z").unwrap().unix_seconds();
    for day in 0..7 {
        let mut lines = String::new();
        for minute in 0..1440 {
            if day == 2 && week.missing.contains(&minute) {
                continue;
            }
            let hot = week.episode_days.contains(&day) && (600..600 + week.minutes).contains(&minute);
            let mut row = json!({
                "ts": Timestamp::from_unix_seconds(start + day * 86_400 + minute * 60).iso(),
                "mem_available_mib": if hot && week.kind == Kind::Mem { 400 } else { 2500 + minute % 7 },
                "swap_used_mib": 40, "zram_used_mib": 20, "zram_compressed_mib": 7,
                "psi_some_avg60": if hot { 4.0 } else { 0.0 },
                "psi_full_avg60": if hot && week.kind == Kind::Psi { 2.5 } else { 0.0 },
                "agents": if hot { 5 } else { 3 }, "agent_pss_mib": if hot { 2100 } else { 1200 },
            });
            if week.steal_minutes > 0 {
                row["steal_pct"] = json!(if day == 2 && (60..60 + week.steal_minutes).contains(&minute) { 25.5 } else { 20.0 });
            }
            lines.push_str(&format!("{row}\n"));
        }
        let date = Timestamp::from_unix_seconds(start + day * 86_400).iso_date();
        fs::write(dir.join(format!("{date}.jsonl")), lines).unwrap();
    }
}

fn run_report(root: &Path, week: &Week, journal: &str) -> String {
    let dir = root.join("rows");
    if !dir.exists() {
        write_week(&dir, week);
    }
    let journal_path = root.join("journal");
    fs::write(&journal_path, journal).unwrap();
    let now = Timestamp::parse("2026-10-14T00:00:00Z").unwrap().unix_seconds_f64();
    let inputs = Inputs { dir, proc_root: root.join("proc"), zram: root.join("zram"), now, journal: Some(journal_path) };
    let mut out = Vec::new();
    report(&inputs, 7.0, &mut out);
    String::from_utf8(out).unwrap()
}

fn verdict(week: &Week, journal: &str) -> (String, String) {
    let scratch = tempfile::tempdir().unwrap();
    let text = run_report(scratch.path(), week, journal);
    let verdict = text.lines().find_map(|line| line.strip_prefix("verdict: ")).unwrap().split(' ').next().unwrap().to_owned();
    (verdict, text)
}

#[test]
fn recorded_weeks_keep_their_upgrade_and_stay_verdicts() {
    let cases = [
        ("15-minute PSI-full episode on 3 days", Week { episode_days: &[1, 3, 5], minutes: 15, kind: Kind::Psi, ..QUIET }, "", "UPGRADE"),
        ("15-minute MemAvailable < 600 MiB on 3 days", Week { episode_days: &[0, 2, 6], minutes: 15, kind: Kind::Mem, ..QUIET }, "", "UPGRADE"),
        ("quiet week", QUIET, "", "STAY"),
        ("episodes on only 2 days", Week { episode_days: &[1, 3], minutes: 15, ..QUIET }, "", "STAY"),
        ("9-minute episodes on 3 days", Week { episode_days: &[1, 3, 5], minutes: 9, ..QUIET }, "", "STAY"),
        ("OOM kill of claude in the window", QUIET, AGENT_JOURNAL, "UPGRADE"),
        ("OOM kills of non-agents", QUIET, OTHER_JOURNAL, "STAY"),
    ];
    for (name, week, journal, expected) in cases {
        let (verdict, text) = verdict(&week, journal);
        assert_eq!(verdict, expected, "{name}\n{text}");
    }
}

#[test]
fn oom_kills_count_agents_and_skip_lines_outside_the_window() {
    let (_, agent) = verdict(&QUIET, AGENT_JOURNAL);
    assert!(agent.lines().any(|line| line == "OOM kills: 1 (1 hit an agent)"), "{agent}");
    assert!(agent.contains("  2026-10-10T10:00:01+00:00 claude [agent]\n"), "{agent}");
    let (_, other) = verdict(&QUIET, OTHER_JOURNAL);
    assert!(other.lines().any(|line| line == "OOM kills: 2 (0 hit an agent)"), "{other}");
}

#[test]
fn report_lines_summarize_the_psi_week() {
    let (_, text) = verdict(&Week { episode_days: &[1, 3, 5], minutes: 15, kind: Kind::Psi, ..QUIET }, "");
    let lines = text.lines().collect::<BTreeSet<_>>();
    for expected in [
        "window: last 7 days, 10080 rows covering 7.00 days (2026-10-07T00:00:00Z .. 2026-10-13T23:59:00Z)",
        "agent CLIs: p50 3 p95 3 max 5",
        "agent PSS MiB: p50 1200 p95 1200 max 2100",
        "min MemAvailable MiB: 2500",
        "minutes with PSI full avg60 > 1%: 45 (some: 45)",
        "minutes with steal > 20%: 0 (of 0 rows with steal sampled)",
        "longest row gap: 60 s ending 2026-10-07T00:01:00Z",
    ] {
        assert!(lines.contains(expected), "missing {expected:?}\n{text}");
    }
}

#[test]
fn report_counts_steal_minutes_over_20_percent_and_the_longest_row_gap() {
    let (verdict, text) = verdict(&Week { steal_minutes: 7, missing: 700..711, ..QUIET }, "");
    assert_eq!(verdict, "STAY", "{text}");
    assert!(text.contains("\nminutes with steal > 20%: 7 (of 10069 rows with steal sampled)\n"), "{text}");
    assert!(text.contains("\nlongest row gap: 720 s ending 2026-10-09T11:51:00Z\n"), "{text}");
}

#[test]
fn an_empty_window_prints_stay_with_no_data() {
    let scratch = tempfile::tempdir().unwrap();
    fs::create_dir_all(scratch.path().join("rows")).unwrap();
    let text = run_report(scratch.path(), &QUIET, "");
    assert!(text.ends_with("verdict: STAY (no data)\n"), "{text}");
}
