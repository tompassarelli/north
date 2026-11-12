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
const SUB_SECOND_BUDGET: &str = r"[(,]\s*Duration::from_millis\(\s*\d{1,3}\s*\)\s*[,)]";
const INJECTED_CLOCK: &str = r"&\|\|";
const SUB_SECOND_BUDGET_TESTS: [(&str, &str); 2] = [
    ("crates/north-fleet/src/fleet/rebooted_tests.rs", "the revive is held until its release, so the 100 ms budget only bounds how promptly the pass returns and the deferral does not depend on it"),
    ("crates/north-capacity-host/src/capacity/watchdog/sampler_tests.rs", "a negative wait: load can only end the 200 ms wait later, never make the process left alone exit, so the assertion holds at any budget"),
];

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn found_in(path: &str, text: &str, pattern: &Regex) -> Vec<String> {
    let quoted = |line: &str, at: usize| line[..at].contains('"');
    text.lines().enumerate().filter_map(|(number, line)| pattern.find(line).filter(|hit| !quoted(line, hit.start())).map(|hit| format!("{path}:{} {}", number + 1, hit.as_str()))).collect()
}

fn rust_files() -> Vec<String> {
    if !north_base::rust_files::is_checkout(root()) {
        return north_base::rust_files::under(root());
    }
    let listed = Command::new("git").current_dir(root()).args(["ls-files", "-z", "--", "*.rs"]).output().unwrap();
    String::from_utf8(listed.stdout).unwrap().split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

fn found(paths: &[String], pattern: &Regex) -> Vec<String> {
    paths.iter().flat_map(|path| found_in(path, &std::fs::read_to_string(root().join(path)).unwrap_or_default(), pattern)).collect()
}

fn sub_second_budgets(path: &str, text: &str) -> Vec<String> {
    let clock = Regex::new(INJECTED_CLOCK).unwrap();
    let unclocked: String = text.lines().map(|line| if clock.is_match(line) { "" } else { line }).collect::<Vec<_>>().join("\n");
    found_in(path, &unclocked, &Regex::new(SUB_SECOND_BUDGET).unwrap())
}

fn found_sub_second(paths: &[String]) -> Vec<String> {
    paths.iter().flat_map(|path| sub_second_budgets(path, &std::fs::read_to_string(root().join(path)).unwrap_or_default())).collect()
}

fn duration_threshold() -> Regex {
    Regex::new(&format!(r"assert!\([^;,]*?\b{MEASURED}\s*(<=?|>=?)\s*[^=]|assert!\([^;,]*?[^=<>-](<=?|>=?)\s*(\d+\s*\*\s*)?{MEASURED}\b")).unwrap()
}

#[test]
fn no_land_check_or_test_reads_wall_time_or_host_load() {
    let rust = rust_files();
    assert!(rust.len() > 100, "listed {} Rust files", rust.len());
    let test_code = Regex::new(TEST_CODE).unwrap();
    let tests: Vec<String> = rust.iter().filter(|path| test_code.is_match(path) && !path.starts_with(RECORDED_LOGS) && *path != THIS_FILE).cloned().collect();
    let checks: Vec<String> = rust.iter().filter(|path| JUDGED_BY_COST.iter().any(|check| path.starts_with(check)) && !test_code.is_match(path)).cloned().collect();
    assert!(!checks.is_empty(), "no source under {JUDGED_BY_COST:?}");
    let timed_tests = found(&tests, &Regex::new(TEST_TIMING).unwrap());
    assert!(timed_tests.is_empty(), "test code reads the wall clock or the host's load; wait on the event through north_base::platform::wait or inject the clock:\n{}", timed_tests.join("\n"));
    let judged: Vec<String> = tests.iter().filter(|path| path.as_str() != REAL_BUDGETS.0).cloned().collect();
    let thresholds = found(&judged, &duration_threshold());
    assert!(thresholds.is_empty(), "a test compares a measured duration to a threshold, so it passes or fails on host load; decide by events through an injected clock or sampler:\n{}", thresholds.join("\n"));
    let budgeted: Vec<String> = tests.iter().filter(|path| !SUB_SECOND_BUDGET_TESTS.iter().any(|(allowed, _)| path.as_str() == *allowed)).cloned().collect();
    let budgets = found_sub_second(&budgeted);
    assert!(budgets.is_empty(), "a test hands the code under test a sub-second wall-clock budget, so it passes or fails on host load; decide by events through an injected clock or raise the budget:\n{}", budgets.join("\n"));
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

const OLD_REAPER_TEST: &str = r#"    let reaped = reap_within(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true, Duration::from_millis(300));
"#;

const SPARED_REAPER_TEST: &str = r#"    let reaped = reap_within(&fleet, &Sight::default(), 24 * HOUR, now_ms(), true, Duration::from_millis(300), &|| Duration::ZERO);
"#;

#[test]
fn the_wall_time_scan_reports_a_sub_second_budget_handed_to_the_code_under_test() {
    let path = "crates/north-fleet-supervisor/src/fleet/reaper_tests.rs";
    let reported = sub_second_budgets(path, OLD_REAPER_TEST);
    assert_eq!(reported.len(), 1, "{reported:?}");
    let spared = sub_second_budgets(path, SPARED_REAPER_TEST);
    assert!(spared.is_empty(), "{spared:?}");
}
