//! On 2026-11-08 three land-time races passed or failed on the host's load: the land_offload ship ordering test,
//! the threads_ledger spool flush test and the strategy replay's budget; on 2026-11-11 a master-probe test did so by
//! comparing a measured duration to a threshold (north#892).
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use regex::Regex;

const TEST_CODE: &str = r"(^|/)(tests?|fixtures)/|(^|[/._])tests?\.[^/]*$";
const RECORDED_LOGS: &str = "tests/fixtures/";
const TEST_TIMING: &str = r"sleep\(|Instant::now\(|\.elapsed\(\)|/proc/loadavg|/proc/pressure|wait::timed\(";
const MEASURED: &str = r"(took|elapsed|duration|latency|wall|(stage|own|wall|run|spent)_(ms|secs))";
const THIS_FILE: &str = "crates/north-land-checks/src/land/wall_time_tests.rs";
const REAL_BUDGETS: (&str, &str) = ("crates/north-providers/src/providers/claude/fleet_tick_tests.rs", "the tick measures hung stages against real budgets through Instant deadlines in north-fleet-supervisor tick.rs; an injected tick clock is north#892 follow-up");
const SUB_SECOND_BUDGET: &str = r"[(,]\s*Duration::from_millis\(\s*\d{1,3}\s*\)\s*[,)]";
const INJECTED_CLOCK: &str = r"&\|\|";
const SUB_SECOND_BUDGET_TESTS: [(&str, &str); 3] = [
    ("crates/north-base/src/platform/process.rs", "a negative wait: load can only end the 200 ms wait later, never make the gated command finish, so the assertion holds at any budget"),
    ("crates/north-fleet/src/fleet/rebooted_tests.rs", "the revive is held until its release, so the 100 ms budget only bounds how promptly the pass returns and the deferral does not depend on it"),
    ("crates/north-capacity-host/src/capacity/watchdog/sampler_tests.rs", "a negative wait: load can only end the 200 ms wait later, never make the process left alone exit, so the assertion holds at any budget"),
];

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn found_in(path: &str, text: &str, pattern: &Regex) -> Vec<String> {
    if !pattern.is_match(text) {
        return Vec::new();
    }
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

fn inline_test_modules(text: &str) -> String {
    static MODULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(pub(\([a-z]+\))? )?mod \w+ \{$").unwrap());
    let (mut armed, mut inside) = (false, false);
    let kept: Vec<&str> = text
        .lines()
        .map(|line| {
            if inside {
                inside = line != "}";
                return line;
            }
            if line == "#[cfg(test)]" || (armed && line.starts_with("#[")) {
                armed = true;
                return "";
            }
            inside = armed && MODULE.is_match(line);
            armed = false;
            if inside { line } else { "" }
        })
        .collect();
    kept.join("\n")
}

fn test_text(path: &str) -> String {
    let text = std::fs::read_to_string(root().join(path)).unwrap_or_default();
    static TEST_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(TEST_CODE).unwrap());
    if TEST_PATH.is_match(path) { text } else { inline_test_modules(&text) }
}

fn found_in_tests<'a>(tests: impl IntoIterator<Item = &'a (String, String)>, pattern: &Regex) -> Vec<String> {
    tests.into_iter().flat_map(|(path, text)| found_in(path, text, pattern)).collect()
}

fn found_sub_second<'a>(tests: impl IntoIterator<Item = &'a (String, String)>) -> Vec<String> {
    let (clock, budget) = (Regex::new(INJECTED_CLOCK).unwrap(), Regex::new(SUB_SECOND_BUDGET).unwrap());
    let unclocked = |text: &str| text.lines().map(|line| if clock.is_match(line) { "" } else { line }).collect::<Vec<_>>().join("\n");
    tests.into_iter().filter(|(_, text)| budget.is_match(text)).flat_map(|(path, text)| found_in(path, &unclocked(text), &budget)).collect()
}

fn duration_threshold() -> Regex {
    Regex::new(&format!(r"assert!\([^;,]*?\b{MEASURED}\s*(<=?|>=?)\s*[^=]|assert!\([^;,]*?[^=<>-](<=?|>=?)\s*(\d+\s*\*\s*)?{MEASURED}\b")).unwrap()
}

#[test]
fn no_land_check_or_test_reads_wall_time_or_host_load() {
    let rust = rust_files();
    assert!(rust.len() > 100, "listed {} Rust files", rust.len());
    let test_code = Regex::new(TEST_CODE).unwrap();
    let tests: Vec<(String, String)> = rust.iter().filter(|path| !path.starts_with(RECORDED_LOGS) && *path != THIS_FILE).map(|path| (path.clone(), test_text(path))).collect();
    let inline = tests.iter().filter(|(path, text)| !test_code.is_match(path) && !text.trim().is_empty()).count();
    assert!(inline > 20, "only {inline} files with an inline test module were scanned");
    let timed_tests = found_in_tests(&tests, &Regex::new(TEST_TIMING).unwrap());
    assert!(timed_tests.is_empty(), "test code reads the wall clock or the host's load; wait on the event through north_base::platform::wait or inject the clock:\n{}", timed_tests.join("\n"));
    let judged = tests.iter().filter(|(path, _)| path.as_str() != REAL_BUDGETS.0);
    let thresholds = found_in_tests(judged, &duration_threshold());
    assert!(thresholds.is_empty(), "a test compares a measured duration to a threshold, so it passes or fails on host load; decide by events through an injected clock or sampler:\n{}", thresholds.join("\n"));
    let budgeted = tests.iter().filter(|(path, _)| !SUB_SECOND_BUDGET_TESTS.iter().any(|(allowed, _)| path.as_str() == *allowed));
    let budgets = found_sub_second(budgeted);
    assert!(budgets.is_empty(), "a test hands the code under test a sub-second wall-clock budget, so it passes or fails on host load; decide by events through an injected clock or raise the budget:\n{}", budgets.join("\n"));
}
