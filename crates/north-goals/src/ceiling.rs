//! The debt a landing may not add to: a debt-allowed goal that reads over its ceiling and names the paths that pay it
//! (`pays` in goals.toml) refuses every landing that touches none of them. A goal that names no paths is environmental,
//! like the machine's load, and no landing can pay it, so it never blocks one.
use std::path::Path;

use super::oracle;
use super::registry::{self, Class};
use super::verdict::{Verdict, judge};

/// The lines naming each paid-by-paths debt goal that `registry` (the base's goals.toml) reads over its ceiling at `root` while none of `changed` lies under its paths.
pub fn unpaid(root: &Path, registry: &str, changed: &[String]) -> Result<Vec<String>, String> {
    let goals = registry::parse(registry).map_err(|problems| problems.join("; "))?;
    let owed: Vec<_> = goals.iter().filter(|goal| goal.class == Class::Debt && goal.debt.as_ref().is_some_and(|debt| !debt.pays.is_empty() && !changed.iter().any(|file| debt.pays.iter().any(|path| file.starts_with(path.as_str()))))).collect();
    let oracles: Vec<(&Path, &str)> = owed.iter().map(|goal| (root, goal.oracle.as_str())).collect();
    Ok(owed.iter().zip(oracle::measure_all(&oracles)).filter_map(|(goal, reading)| match judge(goal, reading) {
        Verdict::RedOverCeiling(value) => goal.debt.as_ref().map(|debt| format!("{} reads {value} {}, over its ceiling {} {}, read by `{}`; this landing touches none of {}", goal.name, goal.unit, debt.ceiling, goal.unit, goal.oracle, debt.pays.join(", "))),
        _ => None,
    }).collect())
}
