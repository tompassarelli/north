//! What a candidate may not change: the evaluator, the rule that settles an arm, the ledger's counting
//! and recording code, the scoring bars, any test, and every harness/defaults.toml key one of those
//! reads. A candidate that touches one is refused before it is scored.
use north_base::platform::toml_json;
use serde_json::{Map, Value};

pub const DEFAULTS: &str = "harness/defaults.toml";
pub const EVALUATOR: &[&str] = &["crates/north-climb/", "crates/north-replay/", "crates/north-strategy-types/"];
pub const WINNER: &[&str] = &["crates/north-pace/src/router/settle.rs", "crates/north-dispatch/src/dispatch/shares.rs", "crates/north-dispatch/src/tune/cells.rs"];
pub const COUNTING: &[&str] = &["crates/north-providers/src/ledger/", "crates/north-threads/", "src/cli/worker_ledger/", "src/cli/ledger.rs", "crates/north-goals/", "src/cli/scoreboard/"];
const SYMBOLS: &[&str] = &["ARM_RUNS", "ARM_WINDOW", "replay_outcome_shift", "replay_landing_p90", "replay_five_hour", "replay_week_floor", "replay_budget", "settle", "fn tiered", "fn efforted", "fn other_arm"];
const BARS: &[&str] = &["harness/goals.toml", "harness/loops.toml", "harness/corrections.toml", "docs/rationale/experiments.toml", "structure.toml"];

pub fn is_test(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    name == "tests.rs" || name.ends_with("_tests.rs") || path.split('/').any(|part| part == "tests" || part == "fixtures")
}

/// What `path` is when a candidate may not change it.
fn guarded(path: &str) -> Option<&'static str> {
    let prefixed = |group: &[&str]| group.iter().any(|prefix| path.starts_with(prefix));
    if prefixed(EVALUATOR) {
        Some("the evaluator")
    } else if prefixed(WINNER) {
        Some("the rule that picks an arm's winner")
    } else if prefixed(COUNTING) {
        Some("the ledger's counting code")
    } else if BARS.contains(&path) {
        Some("a scoring bar")
    } else if is_test(path) {
        Some("a test")
    } else {
        None
    }
}

/// Why each touched path is outside what a candidate may change; empty when every path is allowed.
/// `defaults` is harness/defaults.toml before and after the change: a change to it is judged key by key
/// with `key_refusals`, and refused whole when the contents are not given.
pub fn refusals(touches: &[String], defaults: Option<(&str, &str)>) -> Vec<String> {
    let mut found = Vec::new();
    for path in touches {
        if path == DEFAULTS {
            match defaults {
                Some((before, after)) => found.extend(key_refusals(before, after)),
                None => found.push(format!("{path} changes with no contents to judge its keys by, which a candidate may not do")),
            }
        } else if let Some(why) = guarded(path) {
            found.push(format!("{path} is {why}, which a candidate may not change"));
        }
    }
    found
}

/// Why each harness/defaults.toml key whose table differs between `before` and `after` is refused: one
/// whose `consumers` include a path a candidate may not change sets a bar the candidate is judged by.
pub fn key_refusals(before: &str, after: &str) -> Vec<String> {
    let parse = |text: &str| match toml_json::parse(text) {
        Ok(Value::Object(table)) => table,
        _ => Map::new(),
    };
    let (before, after) = (parse(before), parse(after));
    let mut keys: Vec<&str> = before.keys().chain(after.keys()).map(String::as_str).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter(|key| before.get(*key) != after.get(*key))
        .filter_map(|key| {
            let reader = [before.get(key), after.get(key)].into_iter().flatten().flat_map(consumers).find_map(|path| guarded(&path).map(|why| (path, why)))?;
            Some(format!("{DEFAULTS} key {key} is read by {} ({}), which a candidate may not change", reader.0, reader.1))
        })
        .collect()
}

fn consumers(table: &Value) -> Vec<String> {
    table["consumers"].as_array().into_iter().flatten().filter_map(|path| path.as_str().map(str::to_owned)).collect()
}

/// Why a generated diff is refused for the lines it changes: one that names the replay bars, the arm
/// trial's length, the settle rule or the functions that route an arm, which a candidate may not move
/// even in a file it may edit.
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
