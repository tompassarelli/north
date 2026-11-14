//! Whether a landed commit turned a contract concern red: each contract's answer in one tree, green, red or no verdict when its
//! oracle faults, and the first commit of a landed range at which a concern is red, so the after-landing run can weigh that commit
//! against its parent.
use std::path::Path;

use super::oracle::{self, Fault};
use super::registry::{self, Class, Concern};

#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    Green(f64),
    Red(f64),
    NoVerdict(Fault),
}

impl Answer {
    pub fn red(&self) -> bool {
        matches!(self, Self::Red(_))
    }
}

/// The contract concerns that the text of a `concerns.toml` names.
pub fn contracts(text: &str) -> Result<Vec<Concern>, String> {
    let concerns = registry::parse(text).map_err(|problems| problems.join("; "))?;
    Ok(concerns.into_iter().filter(|concern| concern.class == Class::Contract).collect())
}

/// What the oracle of each of `concerns` answers in the tree at `root`, side by side.
pub fn answers(root: &Path, concerns: &[Concern]) -> Vec<Answer> {
    let oracles: Vec<(&Path, &str)> = concerns.iter().map(|concern| (root, concern.oracle.as_str())).collect();
    concerns
        .iter()
        .zip(oracle::read_all(&oracles))
        .map(|(concern, reading)| match reading {
            Ok(value) if concern.bound.holds(value) => Answer::Green(value),
            Ok(value) => Answer::Red(value),
            Err(fault) => Answer::NoVerdict(fault),
        })
        .collect()
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
