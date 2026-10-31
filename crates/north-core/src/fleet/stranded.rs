//! A lane a stopped fleet session left holding unlanded work: the session's last report stopped for
//! capacity or ended on a pending wait, and a worktree that report names holds commits not on
//! origin/main or uncommitted changes. Dispatch revives such a session once its host frees, and
//! `north flow` lists it until then.
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::model::Machine;

static CAPACITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bStop:\s*capacity\b").expect("capacity stop pattern"));
static DONE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*Done:").expect("done pattern"));

/// Why the session stopped short of landing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stop {
    /// Its report names `Stop: capacity`.
    Capacity,
    /// Its reply ended on a wait, with no final report (`wait::ended_on_a_wait`).
    Wait,
}

impl Stop {
    pub fn label(self) -> &'static str {
        match self {
            Stop::Capacity => "Stop: capacity",
            Stop::Wait => "a pending wait",
        }
    }
}

/// The reply opens with `Done:`, the report of a session that finished.
pub fn reported_done(report: &str) -> bool {
    DONE.is_match(report.trim_start())
}

/// The stop a reply names: `Stop: capacity`, else a reply that ended on a wait; a `Done:` report
/// names neither.
pub fn stop(report: &str) -> Option<Stop> {
    if reported_done(report) {
        return None;
    }
    if CAPACITY.is_match(report) {
        return Some(Stop::Capacity);
    }
    super::wait::ended_on_a_wait(report).then_some(Stop::Wait)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// A repository's worktrees as `git worktree list --porcelain` gives them, its main tree first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Checkout {
    pub worktrees: Vec<Worktree>,
}

impl Checkout {
    pub fn parse(porcelain: &str) -> Self {
        let worktrees = porcelain
            .split("\n\n")
            .filter_map(|block| {
                let path = block.lines().find_map(|line| line.strip_prefix("worktree "))?;
                let branch = block.lines().find_map(|line| line.strip_prefix("branch refs/heads/")).map(str::to_owned);
                Some(Worktree { path: path.into(), branch })
            })
            .collect();
        Self { worktrees }
    }

    /// The repository's name: the directory that holds its main tree, as in `~/code/north/main`.
    pub fn repo(&self) -> Option<String> {
        let root = &self.worktrees.first()?.path;
        Some(root.parent()?.file_name()?.to_string_lossy().into_owned())
    }

    /// The linked worktrees the report names by path, directory or branch.
    pub fn named(&self, report: &str) -> Vec<&Worktree> {
        self.worktrees.iter().skip(1).filter(|tree| names(report, tree)).collect()
    }
}

fn names(report: &str, tree: &Worktree) -> bool {
    let directory = tree.path.file_name().map(|name| name.to_string_lossy().into_owned());
    let words = [Some(tree.path.to_string_lossy().into_owned()), directory, tree.branch.clone()];
    words.into_iter().flatten().filter(|word| distinctive(word)).any(|word| mentions(report, &word))
}

fn distinctive(word: &str) -> bool {
    word.len() >= 4 && word.chars().any(|c| c.is_ascii_digit() || c == '-' || c == '/')
}

fn mentions(text: &str, word: &str) -> bool {
    let part = |c: char| c.is_ascii_alphanumeric() || "-_./".contains(c);
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(part) && !after.is_some_and(|c| part(c) && c != '.' && c != '/')
    })
}

/// A worktree's standing against origin/main.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Lane {
    pub path: PathBuf,
    pub branch: Option<String>,
    /// Commits on it that origin/main does not contain.
    pub ahead: u32,
    /// Uncommitted or untracked changes.
    pub dirty: bool,
    /// Merging it into origin/main changes nothing: its change landed, perhaps in another form.
    pub landed: bool,
}

impl Lane {
    pub fn unlanded(&self) -> bool {
        self.dirty || (self.ahead > 0 && !self.landed)
    }

    pub fn label(&self) -> String {
        self.branch.clone().unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Reads worktrees and their standing on the machine a session's lanes live on.
pub trait Lanes {
    /// The checkout's worktrees, after fetching origin/main there.
    fn checkout(&self, machine: Option<&Machine>, root: &Path) -> Option<Checkout>;
    /// The standing of each of `trees` that reads, in one pass on the machine.
    fn lanes(&self, machine: Option<&Machine>, trees: &[Worktree]) -> Vec<Lane>;
}

/// A stopped session whose last report stopped short of landing, and the lanes that report names.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stranded {
    pub name: String,
    pub provider: String,
    pub account: String,
    pub status: String,
    pub repo: Option<String>,
    pub tasks: Vec<String>,
    /// The machine its conversation and lanes live on, when not the registry's own host.
    pub machine: Option<String>,
    pub stop: Stop,
    /// When its last turn ended.
    pub ended_ms: u64,
    pub lanes: Vec<Lane>,
}

impl Stranded {
    /// Every lane it names holds nothing origin/main lacks.
    pub fn landed(&self) -> bool {
        !self.lanes.iter().any(Lane::unlanded)
    }
}

#[cfg(test)]
#[path = "stranded_tests.rs"]
mod tests;
