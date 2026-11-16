use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use super::candidate::{git, output};
use super::lander::Lander;
use super::queue::{self, RedFix, Ticket, rfc3339};
use crate::landing::red_check::Failure;

pub fn is_revert(message: &str) -> bool {
    message.starts_with("Revert ") || message.lines().any(|line| line.starts_with("Sheriff-Reverts:"))
}

pub fn names(cases: &[Failure]) -> String {
    cases.iter().map(Failure::label).collect::<Vec<_>>().join(", ")
}

pub fn without(worktree: &Path, head: &str, lane: &str) -> Result<bool, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "--detach", head])?;
    if output(worktree, &["revert", "--no-commit", lane])?.status.success() {
        return Ok(true);
    }
    let _ = output(worktree, &["revert", "--abort"]);
    git(worktree, &["reset", "-q", "--hard", head])?;
    Ok(false)
}

/// Drops from the index of `worktree`, a revert staged and not yet committed, every host-case entry naming a test the revert removes, so main never holds a build entry for a test it lacks; a prune that fails is reported and the revert still queues, since a stale host case is better than a red main left standing.
pub fn follow_removed_tests(worktree: &Path) {
    if let Err(error) = prune_removed_tests(worktree) {
        println!("north land: the revert in {} keeps its host-case entries: {error}", worktree.display());
    }
}

fn prune_removed_tests(worktree: &Path) -> Result<(), String> {
    let pruned = Command::new(super::after::follow::program()).args(["buck", "host-cases", "--prune"]).current_dir(worktree).stdin(std::process::Stdio::null()).output().map_err(|error| format!("north buck host-cases --prune: {error}"))?;
    if !pruned.status.success() {
        return Err(format!("north buck host-cases --prune: {}", String::from_utf8_lossy(&pruned.stderr).trim()));
    }
    let rewritten: Vec<&str> = std::str::from_utf8(&pruned.stdout).unwrap_or_default().lines().collect();
    if rewritten.is_empty() {
        return Ok(());
    }
    git(worktree, &[&["add", "--"], rewritten.as_slice()].concat())?;
    Ok(())
}

pub fn queue_revert(lander: &Lander, worktree: &Path, branch: &str) -> Result<String, String> {
    queue_revert_in(&lander.repo, &lander.queue, worktree, branch)
}

pub fn queue_revert_in(repo: &Path, queue_dir: &Path, worktree: &Path, branch: &str) -> Result<String, String> {
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned();
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket { origin, root: worktree.to_owned(), host, branch: branch.to_owned(), head: head.clone(), session: None, queued_at: rfc3339(OffsetDateTime::now_utc()), queued_micros: Some(micros), red_fix: Some(RedFix::Marker), keep_lane: true, ..Ticket::default() };
    let path = queue_dir.join(queue::file_name(micros, branch));
    north_base::land_ticket::queued(&path, "the lander queued a revert").map_err(|error| format!("cannot record {}: {error}", path.display()))?;
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(head)
}
