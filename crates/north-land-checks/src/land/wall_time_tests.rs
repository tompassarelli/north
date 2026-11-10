//! On 2026-11-08 three land-time races passed or failed on the host's load: the land_offload ship ordering test,
//! the threads_ledger spool flush test and the strategy replay's budget (north#892).
use std::path::Path;
use std::process::Command;

use regex::Regex;

const TEST_CODE: &str = r"(^|/)(tests?|fixtures)/|(^|[/._])tests?\.[^/]*$";
const RECORDED_LOGS: &str = "tests/fixtures/";
const TEST_TIMING: &str = r"sleep\(|Instant::now\(|\.elapsed\(\)|/proc/loadavg|/proc/pressure";
const JUDGED_BY_COST: [&str; 1] = ["crates/north-replay/src/"];
const COST_TIMING: &str = r"Instant::now\(|\.elapsed\(\)|SystemTime::now\(|/proc/loadavg|/proc/pressure";

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn found(paths: &[String], pattern: &Regex) -> Vec<String> {
    let quoted = |line: &str, at: usize| line[..at].contains('"');
    paths
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(root().join(path)).unwrap_or_default();
            text.lines().enumerate().filter_map(|(number, line)| pattern.find(line).filter(|hit| !quoted(line, hit.start())).map(|hit| format!("{path}:{} {}", number + 1, hit.as_str()))).collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn no_land_check_or_test_reads_wall_time_or_host_load() {
    let listed = Command::new("git").current_dir(root()).args(["ls-files", "-z", "--", "*.rs"]).output().unwrap();
    let rust: Vec<String> = String::from_utf8(listed.stdout).unwrap().split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
    assert!(rust.len() > 100, "git ls-files listed {} Rust files", rust.len());
    let test_code = Regex::new(TEST_CODE).unwrap();
    let tests: Vec<String> = rust.iter().filter(|path| test_code.is_match(path) && !path.starts_with(RECORDED_LOGS)).cloned().collect();
    let checks: Vec<String> = rust.iter().filter(|path| JUDGED_BY_COST.iter().any(|check| path.starts_with(check)) && !test_code.is_match(path)).cloned().collect();
    assert!(!checks.is_empty(), "no source under {JUDGED_BY_COST:?}");
    let timed_tests = found(&tests, &Regex::new(TEST_TIMING).unwrap());
    assert!(timed_tests.is_empty(), "test code reads the wall clock or the host's load; wait on the event through north_base::platform::wait or inject the clock:\n{}", timed_tests.join("\n"));
    let timed_checks = found(&checks, &Regex::new(COST_TIMING).unwrap());
    assert!(timed_checks.is_empty(), "a land check judges a lane by wall time or host load; measure its thread or process CPU instead:\n{}", timed_checks.join("\n"));
}
