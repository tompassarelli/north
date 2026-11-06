//! What a candidate may not change: the evaluator, the ledger's counting and recording code, the
//! scoring bars, and any test. A candidate that touches one is refused before it is scored.

const EVALUATOR: &[&str] = &["crates/north-climb/", "crates/north-replay/", "crates/north-strategy-types/"];
const COUNTING: &[&str] = &["crates/north-core/src/ledger/", "crates/north-threads/", "src/cli/worker_ledger/", "crates/north-goals/", "src/cli/scoreboard/"];
const BARS: &[&str] = &["harness/goals.toml", "harness/loops.toml", "harness/corrections.toml", "docs/rationale/experiments.toml", "structure.toml"];

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
