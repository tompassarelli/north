//! On 2026-11-08 three land-time races passed or failed on the host's load: the land_offload ship ordering test,
//! the threads_ledger spool flush test and the strategy replay's budget; on 2026-11-11 a master-probe test did so by
//! comparing a measured duration to a threshold (north#892).
use std::path::Path;
use std::process::Command;

use regex::Regex;

const TEST_CODE: &str = r"(^|/)(tests?|fixtures)/|(^|[/._])tests?\.[^/]*$";
const RECORDED_LOGS: &str = "tests/fixtures/";
const TEST_TIMING: &str = r"sleep\(|Instant::now\(|\.elapsed\(\)|/proc/loadavg|/proc/pressure|wait::timed\(";
const MEASURED: &str = r"(took|elapsed|duration|latency|wall|(stage|own|wall|run|spent)_(ms|secs))";
const THIS_FILE: &str = "crates/north-land-checks/src/land/wall_time_tests.rs";
const REAL_BUDGETS: (&str, &str) = ("crates/north-providers/src/providers/claude/fleet_tick_tests.rs", "the tick measures hung stages against real budgets through Instant deadlines in north-fleet-supervisor tick.rs; an injected tick clock is north#892 follow-up");
const JUDGED_BY_COST: [&str; 1] = ["crates/north-replay/src/"];
const COST_TIMING: &str = r"Instant::now\(|\.elapsed\(\)|SystemTime::now\(|/proc/loadavg|/proc/pressure";

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn found_in(path: &str, text: &str, pattern: &Regex) -> Vec<String> {
    let quoted = |line: &str, at: usize| line[..at].contains('"');
    text.lines().enumerate().filter_map(|(number, line)| pattern.find(line).filter(|hit| !quoted(line, hit.start())).map(|hit| format!("{path}:{} {}", number + 1, hit.as_str()))).collect()
}

fn found(paths: &[String], pattern: &Regex) -> Vec<String> {
    paths.iter().flat_map(|path| found_in(path, &std::fs::read_to_string(root().join(path)).unwrap_or_default(), pattern)).collect()
}

fn duration_threshold() -> Regex {
    Regex::new(&format!(r"assert!\([^;,]*?\b{MEASURED}\s*(<=?|>=?)\s*[^=]|assert!\([^;,]*?[^=<>-](<=?|>=?)\s*(\d+\s*\*\s*)?{MEASURED}\b")).unwrap()
}

#[test]
fn no_land_check_or_test_reads_wall_time_or_host_load() {
    let listed = Command::new("git").current_dir(root()).args(["ls-files", "-z", "--", "*.rs"]).output().unwrap();
    let rust: Vec<String> = String::from_utf8(listed.stdout).unwrap().split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
    assert!(rust.len() > 100, "git ls-files listed {} Rust files", rust.len());
    let test_code = Regex::new(TEST_CODE).unwrap();
    let tests: Vec<String> = rust.iter().filter(|path| test_code.is_match(path) && !path.starts_with(RECORDED_LOGS) && *path != THIS_FILE).cloned().collect();
    let checks: Vec<String> = rust.iter().filter(|path| JUDGED_BY_COST.iter().any(|check| path.starts_with(check)) && !test_code.is_match(path)).cloned().collect();
    assert!(!checks.is_empty(), "no source under {JUDGED_BY_COST:?}");
    let timed_tests = found(&tests, &Regex::new(TEST_TIMING).unwrap());
    assert!(timed_tests.is_empty(), "test code reads the wall clock or the host's load; wait on the event through north_base::platform::wait or inject the clock:\n{}", timed_tests.join("\n"));
    let judged: Vec<String> = tests.iter().filter(|path| path.as_str() != REAL_BUDGETS.0).cloned().collect();
    let thresholds = found(&judged, &duration_threshold());
    assert!(thresholds.is_empty(), "a test compares a measured duration to a threshold, so it passes or fails on host load; decide by events through an injected clock or sampler:\n{}", thresholds.join("\n"));
    let timed_checks = found(&checks, &Regex::new(COST_TIMING).unwrap());
    assert!(timed_checks.is_empty(), "a land check judges a lane by wall time or host load; measure its thread or process CPU instead:\n{}", timed_checks.join("\n"));
}

const OLD_SSH_MASTER_TEST: &str = r#"    let (health, took) = wait::timed(|| probe(&program, &env, "testhost", &mut Respawned { fixture: &mut fixture, marker, words }));

    assert!(took >= 2 * SLOW, "the stalled master was not sampled twice: {health:?}");
    let Health::Recycled { direct, after: Some(after) } = health else { panic!("the stalled master was not recycled: {health:?}") };
    assert!(after < Duration::from_secs(1), "the recreated master took {after:?}");
    assert!(stage_ms < 15000, "land-check stage took {stage_ms} ms");
"#;

const DECIDED_BY_EVENTS: &str = r#"    assert_eq!(dial.through[..2], [false, false], "{:?}", dial.through);
    assert!(sent.len() <= 2, "{sent:?}");
    assert!(order.iter().all(|line| line.starts_with("builder\t")), "{order:?}");
    assert!(!path.exists(), "the stalled master {stalled} still runs");
"#;

#[test]
fn the_wall_time_scan_reports_a_test_that_compares_a_measured_duration_to_a_threshold() {
    let path = "crates/north-base/src/platform/ssh_master_tests.rs";
    let measuring = found_in(path, OLD_SSH_MASTER_TEST, &Regex::new(TEST_TIMING).unwrap());
    assert_eq!(measuring.len(), 1, "wait::timed was not reported: {measuring:?}");
    let reported = found_in(path, OLD_SSH_MASTER_TEST, &duration_threshold());
    assert_eq!(reported.len(), 2, "{reported:?}");
    let spared = found_in(path, DECIDED_BY_EVENTS, &duration_threshold());
    assert!(spared.is_empty(), "{spared:?}");
}
