//! Leaf issues in the closed-box report: a leaf the lander closed by its own check counts as one closed box and reads
//! graded, and a run that created children of its issue counts as a split, so converting a parent into leaves does not
//! read as closures falling (north#1137, docs/rationale/sub-issue-cutover.md).
use std::collections::{HashMap, HashSet};

use crate::foundation::clock::Timestamp;
use crate::router::pricing::PriceTable;

use super::closed::{ClosedReport, SpendRun, WORKER};

const CLOSING: &str = "Closed by the landing of ";
const AFTER_RUN_S: i64 = 600;

/// A closed issue as the report reads it: its body, the issue it is a child of, when it was created, and the Done-when
/// command the lander's closing comment names as the check that closed it.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Closed {
    pub body: String,
    pub parent: Option<String>,
    pub created: String,
    pub by_check: Option<String>,
}

impl Closed {
    pub fn of_body(body: impl Into<String>) -> Self {
        Self { body: body.into(), ..Self::default() }
    }

    /// The command the lander's closing comment names, read from the comments of an issue.
    pub fn check_named_in<'a>(comments: impl IntoIterator<Item = &'a str>) -> Option<String> {
        comments.into_iter().find_map(|comment| Some(comment.strip_prefix(CLOSING)?.split_once('`')?.1.split_once('`')?.0.to_owned()))
    }

    /// Whether this is a leaf the lander closed by its own Done-when command: no checkbox in the body, and the command
    /// the closing comment names is the one the body names.
    pub fn closed_by_own_check(&self) -> bool {
        crate::threads::claims::boxes(&self.body).next().is_none() && self.by_check.as_deref().is_some_and(|command| crate::platform::box_oracles::done_when_command(&self.body) == Some(command))
    }
}

/// An open issue whose worker runs created children instead of closing it, with what those runs cost.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct SplitBox {
    pub item: String,
    pub usd: f64,
}

/// The runs that created a child of the issue they worked on, from the run's start to ten minutes after its end.
pub fn creators<'a>(runs: &'a [SpendRun], closed: &HashMap<String, Closed>) -> HashSet<&'a str> {
    let at = |text: &str| Timestamp::parse(text).ok().map(Timestamp::unix_seconds);
    runs.iter()
        .filter(|run| run.role == WORKER)
        .filter(|run| {
            let (Some(item), Some(start), Some(end)) = (run.item.as_deref(), at(&run.started), at(&run.ended)) else { return false };
            closed.values().any(|child| child.parent.as_deref() == Some(item) && at(&child.created).is_some_and(|made| made >= start && made <= end + AFTER_RUN_S))
        })
        .map(|run| run.agent.as_str())
        .collect()
}

/// `runs` without those in `creators` on an issue that is not closed, then those: the first are what the report prices as
/// closing or failing to close, the second are splits.
pub fn apart(runs: &[SpendRun], closed: &HashSet<String>, creators: &HashSet<&str>) -> (Vec<SpendRun>, Vec<SpendRun>) {
    runs.iter().cloned().partition(|run| !(creators.contains(run.agent.as_str()) && run.item.as_deref().is_some_and(|item| !closed.contains(item))))
}

impl ClosedReport {
    /// Counts `made`, the runs that created children of an open issue, as splits of that issue: their spend is the
    /// issue's `splits` entry and stays in the window's spend, and is no spend that closed nothing.
    pub fn split(mut self, made: &[SpendRun], since: &str, prices: &PriceTable) -> Self {
        let mut by_item: std::collections::BTreeMap<&str, f64> = std::collections::BTreeMap::new();
        for run in made.iter().filter(|run| run.started.as_str() >= since) {
            if let Some(item) = run.item.as_deref() {
                *by_item.entry(item).or_default() += run.usd(prices);
            }
        }
        self.splits = by_item.into_iter().map(|(item, usd)| SplitBox { item: item.to_owned(), usd }).collect();
        self.spend_usd += made.iter().filter(|run| run.ended.as_str() >= since).map(|run| run.usd(prices)).sum::<f64>();
        self
    }

    /// Marks each closed arm box graded when `closed` holds it as a leaf the lander closed by its own Done-when command.
    pub fn leaves(mut self, closed: &HashMap<String, Closed>) -> Self {
        for arm in self.arms.iter_mut().filter(|arm| arm.closed && closed.get(&arm.item).is_some_and(Closed::closed_by_own_check)) {
            arm.verified = true;
            arm.issue_verified = true;
        }
        self
    }
}
