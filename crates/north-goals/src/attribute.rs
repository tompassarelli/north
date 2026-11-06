//! Which landed commit turned a contract goal red: the contract goals red in one tree, and the first commit of a landed range at
//! which a goal is red, so the after-landing run can revert that commit.
use std::collections::BTreeSet;
use std::path::Path;

use super::oracle;
use super::registry::{self, Class};
use super::verdict::{Verdict, judge};

/// The contract goals that `goals.toml` at `root` reads red there; none when the tree has no `goals.toml`.
pub fn red_contracts(root: &Path) -> Result<BTreeSet<String>, String> {
    let path = root.join(registry::PATH);
    let Ok(text) = std::fs::read_to_string(&path) else { return Ok(BTreeSet::new()) };
    let goals = registry::parse(&text).map_err(|problems| problems.join("; "))?;
    let contracts: Vec<_> = goals.iter().filter(|goal| goal.class == Class::Contract).collect();
    let oracles: Vec<(&Path, &str)> = contracts.iter().map(|goal| (root, goal.oracle.as_str())).collect();
    Ok(contracts.iter().zip(oracle::measure_all(&oracles)).filter(|(goal, reading)| matches!(judge(goal, reading.clone()), Verdict::RedContract(_))).map(|(goal, _)| goal.name.clone()).collect())
}

/// The index of the first of `commits` (oldest first) at which `red_at` holds, given that it holds at the last one and, once true, stays true.
pub fn first_red(commits: &[String], mut red_at: impl FnMut(&str) -> bool) -> Option<usize> {
    let (mut low, mut high) = (0, commits.len().checked_sub(1)?);
    while low < high {
        let middle = low + (high - low) / 2;
        if red_at(&commits[middle]) {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    Some(low)
}
