use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{PoisonError, RwLock};

use super::queue::{self, Ticket};
use super::lane_gates::{self, Lane};
use super::reservation::{self, Attempt, Judged};
use crate::landing::{merge_continue, merge_set, stale_revert};

static WORKTREES: RwLock<()> = RwLock::new(());

fn run(directory: &Path, arguments: &[&str]) -> Result<Output, String> {
    Command::new("git").arg("-C").arg(directory).args(merge_set::driver_arguments()).args(arguments).env("GIT_EDITOR", "true").output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))
}

fn stdout(arguments: &[&str], output: Output) -> Result<String, String> {
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn output(directory: &Path, arguments: &[&str]) -> Result<Output, String> {
    let _shared = WORKTREES.read().unwrap_or_else(PoisonError::into_inner);
    run(directory, arguments)
}

pub fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    stdout(arguments, output(directory, arguments)?)
}

pub fn change_worktrees_alone(repo: &Path, arguments: &[&str]) -> Result<String, String> {
    let _sole = WORKTREES.write().unwrap_or_else(PoisonError::into_inner);
    stdout(arguments, run(repo, arguments)?)
}

fn succeeds(directory: &Path, arguments: &[&str]) -> bool {
    output(directory, arguments).is_ok_and(|output| output.status.success())
}

pub fn lander_worktree(repo: &Path, name: &str) -> Result<PathBuf, String> {
    let checkout = super::repositories::checkout(repo)?;
    let container = checkout.parent().ok_or_else(|| format!("{} has no project container", checkout.display()))?;
    Ok(container.join("worktrees").join(name))
}

pub fn ensure(repo: &Path, worktree: &Path) -> Result<(), String> {
    if !worktree.join(".git").exists() {
        let path = worktree.to_str().ok_or_else(|| format!("{} is not UTF-8", worktree.display()))?;
        git(repo, &["worktree", "add", "-q", "-f", "--detach", path, "HEAD"])?;
    }
    if worktree.join(".git").is_file() && succeeds(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]) {
        git(worktree, &["cherry-pick", "--abort"])?;
    }
    Ok(())
}

pub fn prepare(repo: &Path, worktree: &Path) -> Result<String, String> {
    ensure(repo, worktree)?;
    git(worktree, &["fetch", "-q", "--no-tags", "origin", "refs/heads/main"])?;
    git(worktree, &["rev-parse", "FETCH_HEAD^{commit}"])
}

pub enum Pick {
    Included,
    Decided(String),
    Refused(lane_gates::Refused),
    Undecided(String),
    Deferred(String),
}

pub struct Built {
    pub head: String,
    pub picks: Vec<Pick>,
    pub tips: Vec<String>,
}

impl Built {
    pub fn included(&self) -> usize {
        self.picks.iter().filter(|pick| matches!(pick, Pick::Included)).count()
    }
}

fn has_commit(worktree: &Path, sha: &str) -> bool {
    succeeds(worktree, &["cat-file", "-e", &format!("{sha}^{{commit}}")])
}

fn dropped_refusal(base: &str, commits: &str) -> String {
    format!("lane commits missing from the candidate after the pick onto main@{base}: {commits}; NOT landed.")
}

fn refuse(logs: &Path, name: &str, kind: &str, text: &str) -> Result<Pick, String> {
    fs::create_dir_all(logs).map_err(|error| format!("{}: {error}", logs.display()))?;
    let log = logs.join(format!("{name}.{kind}.log"));
    fs::write(&log, format!("{text}\n")).map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(Pick::Decided(format!("failed {}", log.display())))
}

fn readable(worktree: &Path, ticket: &Ticket) -> bool {
    if has_commit(worktree, &ticket.head) {
        return true;
    }
    let root = ticket.root.to_str().unwrap_or_default();
    let _ = output(worktree, &["fetch", "-q", "--no-tags", root, &format!("refs/heads/{}", ticket.branch)]);
    has_commit(worktree, &ticket.head)
}

fn files(worktree: &Path, base: &str, ticket: &Ticket) -> HashSet<String> {
    let Ok(fork) = git(worktree, &["merge-base", base, &ticket.head]) else { return HashSet::new() };
    let changed: Vec<String> = git(worktree, &["diff", "--name-only", "--no-renames", &fork, &ticket.head]).unwrap_or_default().lines().map(str::to_owned).collect();
    if changed.is_empty() {
        return HashSet::new();
    }
    let mut arguments = vec!["check-attr", "merge", "--"];
    arguments.extend(changed.iter().map(String::as_str));
    let resolved: HashSet<String> = git(worktree, &arguments).unwrap_or_default().lines().filter_map(|line| line.strip_suffix(": merge: set")).map(str::to_owned).collect();
    changed.into_iter().filter(|path| !resolved.contains(path)).collect()
}

fn queued_ahead<'a>(worktree: &Path, base: &str, ticket: &Ticket, earlier: &[&'a Ticket]) -> Option<&'a str> {
    let own = files(worktree, base, ticket);
    let lane = queue::lane(ticket);
    earlier.iter().find(|ahead| queue::lane(ahead) != lane && readable(worktree, ahead) && !files(worktree, base, ahead).is_disjoint(&own)).map(|ahead| ahead.branch.as_str())
}

fn pick(worktree: &Path, base: &str, ticket: &Ticket, earlier: &[&Ticket], logs: &Path, name: &str) -> Result<Pick, String> {
    if !readable(worktree, ticket) {
        return Ok(Pick::Decided("conflict unreadable".to_owned()));
    }
    let cherry = git(worktree, &["cherry", base, &ticket.head])?;
    if !cherry.lines().any(|line| line.starts_with('+')) {
        return Ok(Pick::Decided(format!("landed {base}")));
    }
    let picked_onto = git(worktree, &["rev-parse", "HEAD"])?;
    let picked = output(worktree, &["cherry-pick", "--allow-empty", &format!("{base}..{}", ticket.head)])?;
    let resolved = picked.status.success() || merge_continue::finish(worktree, &worktree.join(merge_set::DEFAULT_CONFIG)).is_ok();
    if !resolved {
        let files = git(worktree, &["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default().lines().collect::<Vec<_>>().join(",");
        let dropped = if files.is_empty() && succeeds(worktree, &["diff", "--cached", "--quiet", "HEAD"]) { git(worktree, &["rev-parse", "-q", "--verify", "CHERRY_PICK_HEAD"]).ok() } else { None };
        if !succeeds(worktree, &["cherry-pick", "--abort"]) {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        }
        if let Some(commit) = dropped {
            return refuse(logs, name, "dropped", &dropped_refusal(base, &commit));
        }
        let files = if files.is_empty() { "cherry-pick" } else { files.as_str() };
        let bare_main = picked_onto == base;
        let with_main = bare_main || !succeeds(worktree, &["merge-tree", "--write-tree", "--quiet", base, &ticket.head]);
        let red_fix = ticket.red_fix.is_some();
        let queued_ahead = if with_main && !red_fix { queued_ahead(worktree, base, ticket, earlier) } else { None };
        return Ok(reservation::judge(&Judged { branch: &ticket.branch, red_fix, bare_main, queued_ahead }, &Attempt::Conflict { files, with_main }));
    }
    let range = format!("{base}..{}", ticket.head);
    if git(worktree, &["rev-list", "--count", &range])? != git(worktree, &["rev-list", "--count", &format!("{picked_onto}..HEAD")])? {
        git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        return refuse(logs, name, "dropped", &dropped_refusal(base, &git(worktree, &["rev-list", "--reverse", &range])?.replace('\n', " ")));
    }
    match merge_continue::generator_moved(worktree, &worktree.join(merge_set::DEFAULT_CONFIG), base, &picked_onto) {
        Ok(None) => {}
        Ok(Some(reason)) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return Ok(Pick::Deferred(format!("deferred: {}: {reason}; it stays queued for the next candidate", ticket.branch)));
        }
        Err(error) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            return refuse(logs, name, "regenerate", &format!("could not read the regenerate rules: {error}; NOT landed."));
        }
    }
    if let Err(error) = merge_continue::refresh(worktree, &worktree.join(merge_set::DEFAULT_CONFIG), &picked_onto) {
        git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
        return refuse(logs, name, "regenerate", &format!("could not regenerate an index the lane leaves stale: {error}; NOT landed."));
    }
    match stale_revert::reverted(worktree, &picked_onto, "HEAD", &ticket.head) {
        Ok(found) if found.is_empty() => {
            let head = git(worktree, &["rev-parse", "HEAD"])?;
            let lane = Lane { worktree, base: &picked_onto, head: &head, author: ticket.session.as_deref() };
            let Some(refused) = lane_gates::judge(&lane, logs, name)? else { return Ok(Pick::Included) };
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            Ok(Pick::Refused(refused))
        }
        Ok(found) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            refuse(logs, name, "stale-revert", &stale_revert::refusal(&found))
        }
        Err(error) => {
            git(worktree, &["reset", "-q", "--hard", &picked_onto])?;
            Ok(Pick::Undecided(format!("cannot check whether {} reverts recent main: {error}", ticket.branch)))
        }
    }
}

pub fn build(worktree: &Path, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], limit: usize, logs: &Path) -> Result<Built, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "--detach", base])?;
    let mut picks = Vec::new();
    let mut tips = Vec::new();
    let mut included = 0;
    let mut decided = HashSet::new();
    for (path, ticket) in tickets {
        if included >= limit {
            break;
        }
        let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("lane");
        let earlier: Vec<&Ticket> = match queued.iter().position(|(queued, _)| queued == path) {
            Some(at) => queued[..at].iter().filter(|(queued, _)| !decided.contains(queued)).map(|(_, ticket)| ticket).collect(),
            None => Vec::new(),
        };
        let pick = pick(worktree, base, ticket, &earlier, logs, name)?;
        included += usize::from(matches!(pick, Pick::Included));
        if matches!(pick, Pick::Included) {
            tips.push(git(worktree, &["rev-parse", "HEAD"])?);
        }
        if matches!(pick, Pick::Decided(_) | Pick::Refused(_)) {
            decided.insert(path);
        }
        picks.push(pick);
    }
    Ok(Built { head: git(worktree, &["rev-parse", "HEAD"])?, picks, tips })
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
mod tests;
