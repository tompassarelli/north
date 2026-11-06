//! What a candidate may not change: the evaluator, the ledger's counting and recording code, the
//! scoring bars, and any test. A candidate that touches one is refused before it is scored.

const EVALUATOR: &[&str] = &["crates/north-climb/", "crates/north-replay/", "crates/north-strategy-types/"];
const WINNER: &[&str] = &["crates/north-core/src/router/"];
const SYMBOLS: &[&str] = &["ARM_RUNS", "ARM_WINDOW", "replay_outcome_shift", "replay_landing_p90", "replay_five_hour", "replay_week_floor", "replay_budget", "fn tiered", "fn efforted", "fn other_arm"];
const COUNTING: &[&str] = &["crates/north-providers/src/ledger/", "crates/north-threads/", "src/cli/worker_ledger/", "crates/north-goals/", "src/cli/scoreboard/"];
const BARS: &[&str] = &["harness/defaults.toml", "harness/goals.toml", "harness/loops.toml", "harness/corrections.toml", "docs/rationale/experiments.toml", "structure.toml"];

fn is_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "tests.rs" || name.ends_with("_tests.rs") || path.split('/').any(|part| part == "tests" || part == "fixtures")
}

/// Why each touched path is outside what a candidate may change; empty when every path is allowed.
pub fn refusals(touches: &[String]) -> Vec<String> {
    touches
        .iter()
        .filter_map(|path| {
            let prefixed = |group: &[&str]| group.iter().any(|prefix| path.starts_with(prefix));
            let why = if prefixed(EVALUATOR) {
                "the evaluator"
            } else if prefixed(WINNER) {
                "the policy that picks an arm's winner"
            } else if prefixed(COUNTING) {
                "the ledger's counting code"
            } else if BARS.contains(&path.as_str()) {
                "a scoring bar"
            } else if is_test(path) {
                "a test"
            } else {
                return None;
            };
            Some(format!("{path} is {why}, which a candidate may not change"))
        })
        .collect()
}

/// Why a generated diff is refused for the lines it changes: one that names the replay bars, the arm trial's length or the functions that pick an arm's winner, which a candidate may not move even in a file it may edit.
pub fn diff_refusals(diff: &str) -> Vec<String> {
    let changed = diff.lines().filter(|line| (line.starts_with('+') || line.starts_with('-')) && !line.starts_with("+++") && !line.starts_with("---"));
    let mut found: Vec<String> = Vec::new();
    for line in changed {
        for symbol in SYMBOLS.iter().filter(|symbol| line.contains(*symbol)) {
            let reason = format!("the diff changes a line naming {symbol}, which sets the bar a candidate is judged against");
            if !found.contains(&reason) {
                found.push(reason);
            }
        }
    }
    found
}
