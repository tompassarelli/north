//! The sheriff's request: the trailers a red after-landing run files on origin for the sheriff to answer, how they are written and read, and the green landing a request bisects from.
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use crate::land::candidate::git;
use super::short;
use crate::landing::red_check::{self, Failure};

pub const REQUESTS: &str = "refs/heads/claude/land-revert-";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub head: String,
    pub parent: String,
    pub run: String,
    pub log: String,
    pub failing: BTreeSet<Failure>,
    pub tree: String,
    pub live_fix: String,
    pub walked_back: String,
}

fn trailer(message: &str, key: &str) -> String {
    message.lines().find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(':'))).map(|value| value.trim().to_owned()).unwrap_or_default()
}

pub fn parse(head: &str, message: &str) -> Request {
    Request { head: head.to_owned(), parent: trailer(message, "Green-parent"), run: trailer(message, "Main-red-run"), log: trailer(message, "Main-red-log"), failing: red_check::failures(message), tree: trailer(message, "Main-red-tree"), live_fix: trailer(message, "Live-fix"), walked_back: trailer(message, "Walked-back") }
}

pub fn request_message(head: &str, parent: &str, failing: &BTreeSet<Failure>, log: &Path, tree: &str) -> String {
    let lines: Vec<String> = failing.iter().map(Failure::line).collect();
    format!("Main red at {head}\n\n{}\n\nGreen-parent: {parent}\nMain-red-run: post-land checks of {}\nMain-red-log: {}\nMain-red-tree: {tree}", lines.join("\n"), short(head), log.display())
}

pub fn green_ancestor(ledger: &str, repository: &str, history: &[&str]) -> Option<String> {
    let green: BTreeSet<&str> = ledger
        .lines()
        .filter_map(|row| {
            let fields: Vec<&str> = row.split('\t').collect();
            (fields.get(1) == Some(&repository) && fields.get(3) == Some(&"green")).then(|| fields[2])
        })
        .collect();
    history.iter().find(|commit| green.contains(**commit)).map(|commit| (*commit).to_owned())
}

pub fn file(repo: &Path, head: &str, parent: &str, walked_back: Option<&str>, failing: &BTreeSet<Failure>, log: &Path) -> Result<(), String> {
    let commit_tree = git(repo, &["rev-parse", &format!("{head}^{{tree}}")])?;
    let tree = crate::land::verdict_tree::named(log, head, &commit_tree);
    let message = request_message(head, parent, failing, log, &tree) + &walked_back.map(|commit| format!("\nWalked-back: {commit}")).unwrap_or_default();
    let request = git(repo, &["commit-tree", &commit_tree, "-p", head, "-m", &message])?;
    let pushed = Command::new("git").arg("-C").arg(repo).args(["push", "-q", "--no-verify", "origin", &format!("{request}:{REQUESTS}{head}")]).env("SAFE_PUSH_ACTIVE", "1").output().map_err(|error| error.to_string())?;
    if pushed.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&pushed.stderr).trim().to_owned()) }
}
