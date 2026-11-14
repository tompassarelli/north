//! The issues the 24-hour closer filed in place of the issues it closed, read from the record only the closer writes (north#1495): a lead whose issue was closed and rewritten launches on its successor, and no text on GitHub widens a grant.

use std::collections::BTreeSet;

/// Reads the closer's record of its rewrites.
pub trait Rewrites: Send + Sync {
    /// Every rewrite the closer recorded, as (closed, successor), each `[OWNER/]REPO#N`.
    fn rewrites(&self) -> Vec<(String, String)>;
}

pub struct NoRewrites;

impl Rewrites for NoRewrites {
    fn rewrites(&self) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// `task` as `repo#n`, owner prefix dropped.
pub(super) fn issue(task: &str) -> String {
    task.rsplit('/').next().unwrap_or(task).to_lowercase()
}

/// Every issue `rows` rewrote `held` into, through each later rewrite of a successor too.
pub(super) fn successors<'a>(rows: &[(String, String)], held: impl Iterator<Item = &'a str>) -> BTreeSet<String> {
    let mut reached: BTreeSet<String> = held.map(issue).collect();
    let mut found = BTreeSet::new();
    loop {
        let next: Vec<String> = rows.iter().filter(|(closed, _)| reached.contains(&issue(closed))).map(|(_, successor)| issue(successor)).filter(|successor| !reached.contains(successor)).collect();
        if next.is_empty() {
            return found;
        }
        found.extend(next.iter().cloned());
        reached.extend(next);
    }
}
