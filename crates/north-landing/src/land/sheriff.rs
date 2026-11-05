use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use super::after;
use super::candidate::{self, git, output};
use super::enqueue::rfc3339;
use super::lander::Lander;
use super::queue::{self, RedFix, Ticket};
use crate::landing::red_check::{self, Failure};

pub const REQUESTS: &str = "refs/heads/claude/land-revert-";
const PROBE_SECONDS: &str = "1800";
const WORKTREE: &str = "lander-sheriff";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub head: String,
    pub parent: String,
    pub run: String,
    pub failing: BTreeSet<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub commit: String,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    Stale(String),
    Unnamed,
    Revert { lane: String, cases: Vec<Failure> },
    Environment { cases: Vec<Failure> },
}

fn trailer(message: &str, key: &str) -> String {
    message.lines().find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(':'))).map(|value| value.trim().to_owned()).unwrap_or_default()
}

pub fn parse(head: &str, message: &str) -> Request {
    Request { head: head.to_owned(), parent: trailer(message, "Green-parent"), run: trailer(message, "Main-red-run"), failing: red_check::failures(message) }
}

pub fn is_revert(message: &str) -> bool {
    message.starts_with("Revert ") || message.lines().any(|line| line.starts_with("Sheriff-Reverts:"))
}

pub fn find(request: &Request, tip: &str, lanes: &[Lane], reverted: &BTreeSet<String>, mut probe: impl FnMut(&Lane) -> Option<BTreeSet<Failure>>) -> Finding {
    if tip != request.head {
        return Finding::Stale(format!("main moved past {} to {tip}; a later landing may already fix it, so nothing is reverted", request.head));
    }
    if request.failing.is_empty() {
        return Finding::Unnamed;
    }
    let cases: Vec<Failure> = request.failing.iter().cloned().collect();
    for lane in lanes {
        if is_revert(&lane.message) || reverted.contains(&lane.commit) {
            continue;
        }
        if probe(lane).is_some_and(|still| still.is_disjoint(&request.failing)) {
            return Finding::Revert { lane: lane.commit.clone(), cases };
        }
    }
    Finding::Environment { cases }
}

pub fn names(cases: &[Failure]) -> String {
    cases.iter().map(Failure::label).collect::<Vec<_>>().join(", ")
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(9)]
}

fn records(lander: &Lander) -> PathBuf {
    lander.queue.join("sheriff")
}

fn reverted(lander: &Lander) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(records(lander)) else { return BTreeSet::new() };
    entries.filter_map(Result::ok).filter_map(|entry| fs::read_to_string(entry.path()).ok()).filter_map(|text| text.strip_prefix("revert ").and_then(|rest| rest.split_whitespace().next()).map(str::to_owned)).collect()
}

fn lanes(worktree: &Path, request: &Request) -> Result<Vec<Lane>, String> {
    let range = format!("{}..{}", request.parent, request.head);
    let log = git(worktree, &["log", "--first-parent", "-z", "--format=%H%n%B", &range])?;
    Ok(log.split('\0').filter_map(|entry| entry.trim_start().split_once('\n')).map(|(commit, message)| Lane { commit: commit.to_owned(), message: message.trim().to_owned() }).collect())
}

fn without(worktree: &Path, head: &str, lane: &str) -> Result<bool, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "--detach", head])?;
    if output(worktree, &["revert", "--no-commit", lane])?.status.success() {
        return Ok(true);
    }
    let _ = output(worktree, &["revert", "--abort"]);
    git(worktree, &["reset", "-q", "--hard", head])?;
    Ok(false)
}

fn probe(worktree: &Path, request: &Request, lane: &Lane) -> Option<BTreeSet<Failure>> {
    let command = red_check::rerun_command(&request.failing)?;
    if !without(worktree, &request.head, &lane.commit).ok()? {
        return None;
    }
    println!("north land: sheriff: running {} on main without {}", names(&request.failing.iter().cloned().collect::<Vec<_>>()), short(&lane.commit));
    let run = Command::new("timeout").args([PROBE_SECONDS, "bash", "-c", &command]).current_dir(worktree).output().ok()?;
    let log = format!("{}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let still = red_check::failures(&log);
    if !run.status.success() && still.is_empty() {
        return None;
    }
    Some(still)
}

fn lane_ticket(lander: &Lander, request: &Request, lane: &str) -> Option<Ticket> {
    let done = fs::read_dir(lander.queue.join("done")).ok()?;
    done.filter_map(Result::ok).filter_map(|entry| queue::read(&entry.path()).ok()).find(|ticket| {
        ticket.verdict.as_deref().is_some_and(|verdict| verdict.starts_with("landed "))
            && (ticket.head == lane || git(&lander.repo, &["cherry", &ticket.head, &request.head, &request.parent]).is_ok_and(|cherry| cherry.lines().any(|line| line == format!("- {lane}"))))
    })
}

pub(super) fn queue_revert(lander: &Lander, worktree: &Path, branch: &str) -> Result<String, String> {
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    let origin = git(&lander.repo, &["remote", "get-url", "--push", "origin"])?;
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned();
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket { origin, root: worktree.to_owned(), host, branch: branch.to_owned(), head: head.clone(), session: None, queued_at: rfc3339(OffsetDateTime::now_utc()), first_commit_secs: None, several_commits: None, red_fix: Some(RedFix::Marker), verdict: None, keep_lane: true, urgent: None, decided_on: None };
    let path = lander.queue.join(queue::file_name(micros, branch));
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(head)
}

fn revert(lander: &Lander, worktree: &Path, request: &Request, lane: &str, cases: &[Failure]) -> Result<String, String> {
    let branch = format!("sheriff-revert-{}", short(lane));
    if !without(worktree, &request.head, lane)? {
        return Err(format!("{} no longer reverts cleanly onto {}", short(lane), short(&request.head)));
    }
    git(worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(worktree, &["log", "-1", "--format=%s", lane])?;
    let message = format!(
        "Revert {subject}\n\nMain went red at {} ({}): {} fail there and pass on main without {}, so the sheriff reverts it through the landing queue.\n\nReverts: {lane}\nSheriff-Reverts: {lane}\nMain-red: {}",
        short(&request.head),
        request.run,
        names(cases),
        short(lane),
        request.run
    );
    git(worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = queue_revert(lander, worktree, &branch)?;
    if let Some(owner) = lane_ticket(lander, request, lane)
        && let Some(session) = owner.session.as_deref().filter(|session| !session.is_empty())
    {
        let text = format!(
            "north land: main went red at {} on {}; main without {} ({}) passes them, so the sheriff queued its revert {branch} ahead of the queue; fix the lane and queue it again with Follows: {}",
            short(&request.head),
            names(cases),
            owner.branch,
            short(lane),
            short(lane)
        );
        if !after::send(session, &text) {
            println!("north land: sheriff: could not tell session {session} that {} was reverted", owner.branch);
        }
    }
    Ok(format!("revert {lane} {head} queued as {branch}"))
}

fn answer(lander: &Lander, worktree: &Path, name: &str, tip: &str) -> Result<String, String> {
    git(worktree, &["fetch", "-q", "--no-tags", "origin", name])?;
    let main = git(&lander.repo, &["ls-remote", "origin", "refs/heads/main"])?.split_whitespace().next().unwrap_or_default().to_owned();
    let head = git(worktree, &["rev-parse", &format!("{tip}^")])?;
    if name.strip_prefix(REQUESTS) != Some(head.as_str()) {
        return Ok(format!("malformed: {name} does not sit on the commit it names"));
    }
    let request = parse(&head, &git(worktree, &["log", "-1", "--format=%B", tip])?);
    if request.parent.is_empty() {
        return Ok(format!("malformed: {name} names no Green-parent"));
    }
    let lanes = if main == head { lanes(worktree, &request)? } else { Vec::new() };
    match find(&request, &main, &lanes, &reverted(lander), |lane| probe(worktree, &request, lane)) {
        Finding::Stale(reason) => Ok(format!("stale {reason}")),
        Finding::Unnamed => Ok("unnamed: the failing run names no test, so nothing is reverted".to_owned()),
        Finding::Environment { cases } => Ok(format!("environment {} still fail on main without each landed lane; nothing is reverted", names(&cases))),
        Finding::Revert { lane, cases } => revert(lander, worktree, &request, &lane, &cases),
    }
}

pub fn patrol(lander: &Lander) -> Result<(), String> {
    let listing = git(&lander.repo, &["ls-remote", "origin", &format!("{REQUESTS}*")])?;
    for (tip, name) in listing.lines().filter_map(|line| line.split_once('\t')) {
        let Some(head) = name.strip_prefix(REQUESTS) else { continue };
        let record = records(lander).join(queue::slug(head));
        if record.exists() {
            continue;
        }
        let worktree = candidate::lander_worktree(&lander.repo, WORKTREE)?;
        candidate::ensure(&lander.repo, &worktree)?;
        let verdict = answer(lander, &worktree, name, tip)?;
        println!("north land: sheriff: main red at {}: {verdict}", short(head));
        queue::write_atomic(&record, &format!("{verdict}\n")).map_err(|error| format!("cannot write {}: {error}", record.display()))?;
        let _ = Command::new("git").arg("-C").arg(&lander.repo).args(["push", "-q", "--no-verify", "origin", "--delete", name]).env("SAFE_PUSH_ACTIVE", "1").output();
    }
    Ok(())
}

#[cfg(test)]
#[path = "sheriff_tests.rs"]
mod tests;
