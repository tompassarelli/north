//! The debt a landing may help pay: a debt-allowed goal that reads over its ceiling and names the paths that pay it
//! (`pays` in goals.toml) moves every landing touching one of them to the front of the landing queue. No landing is
//! refused for the debt, since refusing finished work shortens no wait. A goal that names no paths is environmental,
//! like the machine's load, and no landing can pay it.
use std::path::Path;

use super::oracle;
use super::registry::{self, Class};
use super::verdict::{Verdict, judge};

/// The lines naming each paid-by-paths debt goal that `registry` (the base's goals.toml) reads over its ceiling at `root` while some of `changed` lies under its paths.
pub fn paid(root: &Path, registry: &str, changed: &[String]) -> Result<Vec<String>, String> {
    let goals = registry::parse(registry).map_err(|problems| problems.join("; "))?;
    let paying: Vec<_> = goals.iter().filter(|goal| goal.class == Class::Debt && goal.debt.as_ref().is_some_and(|debt| changed.iter().any(|file| debt.pays.iter().any(|path| file.starts_with(path.as_str()))))).collect();
    let oracles: Vec<(&Path, &str)> = paying.iter().map(|goal| (root, goal.oracle.as_str())).collect();
    Ok(paying.iter().zip(oracle::measure_all(&oracles)).filter_map(|(goal, reading)| match judge(goal, reading) {
        Verdict::RedOverCeiling(value) => goal.debt.as_ref().map(|debt| format!("{} reads {value} {}, over its ceiling {} {}, read by `{}`; this landing touches {}, which pays it", goal.name, goal.unit, debt.ceiling, goal.unit, goal.oracle, debt.pays.join(", "))),
        _ => None,
    }).collect())
}
