//! The Sonnet reviewer's brief, the classes it rejects a commit for, and who lands the commit: the reviewer or the
//! review job.

use super::super::sampling::{self, Verdict};
use std::path::Path;

/// The classes a reviewer rejects a DeepSeek commit for, each a mistake the lander saw from DeepSeek workers on 2026-11-09/10.
pub const CLASSES: [(&str, &str); 9] = [
    ("test-passes-on-base", "an added test passes with the lane's non-test changes reverted"),
    ("test-budget", "the lane grows the test count past the verification budget of 20 s without deleting as many tests"),
    ("restored-revert", "it restores work that a revert on main removed"),
    ("test-reads-host", "a test reads /proc, the wall clock or another live host fact"),
    ("unchecked-tick", "it ticks a box without running that box's check"),
    ("duplicate", "origin/main already holds a commit of the same name"),
    ("oracle-edit", "it edits the file defining a Done-when test that stood before staffing"),
    ("out-of-scope", "it changes behaviour the box does not ask for"),
    ("other-defect", "any other real defect"),
];

/// Who lands a DeepSeek worker's commit. Never the worker: its lane-only shell refuses safe-push (north#1041).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Lander {
    /// A Sonnet reviewer the review job launches on the lane, for a commit `sampling` picks.
    Reviewer,
    /// The review job itself, through safe-push, for a commit `sampling` passes over.
    ReviewJob,
}

impl Lander {
    /// The landing of `sha`, a commit of a box in `category`, by the verdicts so far.
    pub fn of(verdicts: &[Verdict], category: &str, landing: bool, locked: bool, sha: &str) -> Self {
        if sampling::reviews(verdicts, category, landing, locked, sha) { Self::Reviewer } else { Self::ReviewJob }
    }

    fn says(self) -> &'static str {
        match self {
            Self::Reviewer => "a commit sampling picks goes to a Sonnet reviewer, who lands it through safe-push",
            Self::ReviewJob => "dispatch's review job lands any other commit through safe-push itself",
        }
    }
}

/// The order every DeepSeek worker's brief carries, naming each `Lander` the review job may take.
pub fn worker_order() -> String {
    format!(
        "Lane-only: never run safe-push; it refuses at once with `lane-only session: end with Done: <worktree> <branch> <sha>; the review-and-land loop lands it`. \
         Commit in your lane with this brief's `Item:` and `Box:` lines in the commit message, and report Done: with that line. Then {}, and {}; \
         the lander ticks the box from those lines once the commit is on main.",
        Lander::Reviewer.says(),
        Lander::ReviewJob.says()
    )
}

/// The brief of the Sonnet reviewer of `job`'s commit `sha` in `lane`.
pub fn brief(job_name: &str, item: &str, category: &str, box_text: Option<&str>, lane: &Path, sha: &str, effort: &str) -> String {
    let classes: String = CLASSES.iter().map(|(class, what)| format!("- `{class}`: {what}\n")).collect();
    let verdict = format!("north fleet dispatch verdict --session {job_name} --category {category} --sha {sha} --effort {effort}");
    format!(
        "Category: review\nItem: {item}\n{}Review and land the DeepSeek worker {job_name}'s commits in {} (head {sha}) against the box above.\n\n\
         Read `git -C {} log -p origin/main..HEAD` and the box's Done when, and run that check in the lane. Reject the commit for any of these classes:\n{classes}\n\
         If it is clean, land it with `safe-push` from the lane and run `{verdict} clean`. If you fix a real defect yourself, land the fixed lane and run `{verdict} defect CLASS`. \
         If it cannot land, run `{verdict} defect CLASS`, which records the rejection on the lane so `north lane gc` bundles it to the lane archive with your verdict and removes it once you exit, and report Not done naming the class; dispatch then gives the box to Claude.\n",
        box_text.map_or_else(String::new, |text| format!("Box: {text}\n")),
        lane.display(),
        lane.display(),
    )
}
