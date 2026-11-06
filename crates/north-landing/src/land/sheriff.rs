use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use super::candidate::{self, git, output};
use super::enqueue::rfc3339;
use super::lander::Lander;
use super::after::owner::{self, Reverted};
use super::post_land;
use super::queue::{self, RedFix, Ticket};
use super::steps;
use crate::landing::red_check::{self, Failure};
use north_strategy_types::land::{SheriffCase, SheriffStep};

pub const REQUESTS: &str = "refs/heads/claude/land-revert-";
const PROBE_SECONDS: &str = "1800";
const WORKTREE: &str = "lander-sheriff";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub head: String,
    pub parent: String,
    pub run: String,
    pub log: String,
    pub failing: BTreeSet<Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    pub commit: String,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    NoVerdict(String),
    Fixed { cases: Vec<Failure> },
    Revert { lane: String, cases: Vec<Failure> },
    Flaky { lane: String, cases: Vec<Failure> },
    Environment { cases: Vec<Failure> },
}

fn trailer(message: &str, key: &str) -> String {
    message.lines().find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix(':'))).map(|value| value.trim().to_owned()).unwrap_or_default()
}

pub fn parse(head: &str, message: &str) -> Request {
    Request { head: head.to_owned(), parent: trailer(message, "Green-parent"), run: trailer(message, "Main-red-run"), log: trailer(message, "Main-red-log"), failing: red_check::failures(message) }
}

pub fn is_revert(message: &str) -> bool {
    message.starts_with("Revert ") || message.lines().any(|line| line.starts_with("Sheriff-Reverts:"))
}

fn labels(failures: &BTreeSet<Failure>) -> Vec<String> {
    failures.iter().map(|failure| failure.label().to_owned()).collect()
}

fn bisect(case: &mut SheriffCase, failing: &BTreeSet<Failure>, lanes: &[Lane], at: &mut impl FnMut(&Lane) -> Option<BTreeSet<Failure>>) -> Option<usize> {
    let (mut culprit, mut passing) = (0, lanes.len());
    while passing - culprit > 1 {
        let middle = culprit + (passing - culprit) / 2;
        steps::sheriff(case, &SheriffStep::Probe(middle as u64));
        let Some(found) = at(&lanes[middle]) else {
            steps::sheriff(case, &SheriffStep::Unprobed(middle as u64));
            return None;
        };
        case.probes.push((middle as u64, labels(&found)));
        if found.is_disjoint(failing) {
            passing = middle;
        } else {
            culprit = middle;
        }
    }
    steps::sheriff(case, &SheriffStep::Culprit(culprit as u64));
    Some(culprit)
}

pub fn find(
    request: &Request,
    tip: &str,
    lanes: &[Lane],
    reverted: &BTreeSet<String>,
    mut at: impl FnMut(&Lane) -> Option<BTreeSet<Failure>>,
    mut probe: impl FnMut(&Lane) -> Option<BTreeSet<Failure>>,
) -> Finding {
    let mut case = SheriffCase { head: request.head.clone(), tip: tip.to_owned(), failing: labels(&request.failing), lanes: lanes.iter().map(|lane| lane.commit.clone()).collect(), probes: Vec::new(), confirmations: Vec::new() };
    let no_verdict = |case: &SheriffCase, reason: String| {
        steps::sheriff(case, &SheriffStep::NoVerdict);
        Finding::NoVerdict(reason)
    };
    if request.failing.is_empty() {
        return no_verdict(&case, "the red run names no failing step, so no check reached a failing verdict".to_owned());
    }
    let Some(newest) = lanes.first().filter(|newest| newest.commit == tip) else {
        return no_verdict(&case, format!("no landing lies between the last green {} and the tip {}", short(&request.parent), short(tip)));
    };
    steps::sheriff(&case, &SheriffStep::Probe(0));
    let Some(reproduced) = at(newest) else {
        return no_verdict(&case, format!("rerunning {} at the tip {} reached no verdict", names(&request.failing.iter().cloned().collect::<Vec<_>>()), short(tip)));
    };
    case.probes.push((0, labels(&reproduced)));
    let failing: BTreeSet<Failure> = reproduced.intersection(&request.failing).cloned().collect();
    let cases: Vec<Failure> = if failing.is_empty() { request.failing.iter().cloned().collect() } else { failing.iter().cloned().collect() };
    if failing.is_empty() {
        steps::sheriff(&case, &SheriffStep::Fixed);
        return Finding::Fixed { cases };
    }
    let suspects: Vec<usize> = match bisect(&mut case, &failing, lanes, &mut at) {
        Some(culprit) => vec![culprit],
        None => (0..lanes.len()).collect(),
    };
    for index in suspects {
        let lane = &lanes[index];
        if is_revert(&lane.message) || reverted.contains(&lane.commit) {
            continue;
        }
        steps::sheriff(&case, &SheriffStep::Confirm(index as u64));
        let still = probe(lane);
        case.confirmations.push((index as u64, still.as_ref().map(labels)));
        if still.is_some_and(|still| still.is_disjoint(&failing)) {
            steps::sheriff(&case, &SheriffStep::Recheck);
            let Some(again) = at(newest) else {
                return no_verdict(&case, format!("rerunning {} at the tip {} after the confirmation reached no verdict", names(&cases), short(tip)));
            };
            case.probes.push((0, labels(&again)));
            if again.is_disjoint(&failing) {
                steps::sheriff(&case, &SheriffStep::Flaky);
                return Finding::Flaky { lane: lane.commit.clone(), cases };
            }
            steps::sheriff(&case, &SheriffStep::Revert(index as u64));
            return Finding::Revert { lane: lane.commit.clone(), cases };
        }
    }
    steps::sheriff(&case, &SheriffStep::Environment);
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

fn lanes(worktree: &Path, parent: &str, tip: &str) -> Result<Vec<Lane>, String> {
    let log = git(worktree, &["log", "--first-parent", "-z", "--format=%H%n%B", &format!("{parent}..{tip}")])?;
    Ok(log.split('\0').filter_map(|entry| entry.trim_start().split_once('\n')).map(|(commit, message)| Lane { commit: commit.to_owned(), message: message.trim().to_owned() }).collect())
}

pub(crate) fn without(worktree: &Path, head: &str, lane: &str) -> Result<bool, String> {
    git(worktree, &["reset", "-q", "--hard"])?;
    git(worktree, &["checkout", "-q", "--detach", head])?;
    if output(worktree, &["revert", "--no-commit", lane])?.status.success() {
        return Ok(true);
    }
    let _ = output(worktree, &["revert", "--abort"]);
    git(worktree, &["reset", "-q", "--hard", head])?;
    Ok(false)
}

struct Probes<'a> {
    lander: &'a Lander,
    worktree: &'a Path,
    request: &'a Request,
    tip: &'a str,
}

impl Probes<'_> {
    fn tests(&self) -> BTreeSet<Failure> {
        self.request.failing.iter().filter(|failure| matches!(failure, Failure::Cargo(_) | Failure::Harness(_))).cloned().collect()
    }

    fn steps(&self) -> Vec<String> {
        self.request.failing.iter().filter_map(|failure| match failure {
            Failure::Step(command) => Some(command.clone()),
            _ => None,
        }).collect()
    }

    fn rerun(&self, commit: &str, tree: &str) -> Option<BTreeSet<Failure>> {
        let tests = self.tests();
        if tests.is_empty() {
            let steps = self.steps();
            if steps.is_empty() {
                return None;
            }
            println!("north land: sheriff: rerunning {} on {tree}", steps.join(", "));
            return post_land::rerun(self.lander, &self.request.parent, commit, &steps);
        }
        git(self.worktree, &["reset", "-q", "--hard"]).ok()?;
        git(self.worktree, &["checkout", "-q", "--detach", commit]).ok()?;
        println!("north land: sheriff: running {} on {tree}", names(&tests.iter().cloned().collect::<Vec<_>>()));
        let command = red_check::rerun_command(&tests)?;
        let run = Command::new("timeout").args([PROBE_SECONDS, "bash", "-c", &command]).current_dir(self.worktree).output().ok()?;
        let log = format!("{}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
        let still = red_check::failures(&log);
        if !run.status.success() && still.is_empty() {
            return None;
        }
        Some(still)
    }

    fn at(&self, lane: &Lane) -> Option<BTreeSet<Failure>> {
        self.rerun(&lane.commit, &format!("main at {}", short(&lane.commit)))
    }

    fn without(&self, lane: &Lane) -> Option<BTreeSet<Failure>> {
        if !without(self.worktree, self.tip, &lane.commit).ok()? {
            return None;
        }
        let message = format!("sheriff probe: main {} without {}", short(self.tip), short(&lane.commit));
        git(self.worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message]).ok()?;
        let probe = git(self.worktree, &["rev-parse", "HEAD"]).ok()?;
        self.rerun(&probe, &format!("main {} without {}", short(self.tip), short(&lane.commit)))
    }
}

fn lane_ticket(lander: &Lander, request: &Request, tip: &str, lane: &str) -> Option<Ticket> {
    let done = fs::read_dir(lander.queue.join("done")).ok()?;
    done.filter_map(Result::ok).filter_map(|entry| queue::read(&entry.path()).ok()).find(|ticket| {
        ticket.verdict.as_deref().is_some_and(|verdict| verdict.starts_with("landed "))
            && (ticket.head == lane || git(&lander.repo, &["cherry", &ticket.head, tip, &request.parent]).is_ok_and(|cherry| cherry.lines().any(|line| line == format!("- {lane}"))))
    })
}

pub(crate) fn queue_revert(lander: &Lander, worktree: &Path, branch: &str) -> Result<String, String> {
    queue_revert_in(&lander.repo, &lander.queue, worktree, branch)
}

pub(crate) fn queue_revert_in(repo: &Path, queue_dir: &Path, worktree: &Path, branch: &str) -> Result<String, String> {
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned();
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket { origin, root: worktree.to_owned(), host, branch: branch.to_owned(), head: head.clone(), session: None, queued_at: rfc3339(OffsetDateTime::now_utc()), queued_micros: Some(micros), first_commit_secs: None, several_commits: None, red_fix: Some(RedFix::Marker), verdict: None, keep_lane: true, urgent: None, decided_on: None, published: None, unchecked: Vec::new() };
    let path = queue_dir.join(queue::file_name(micros, branch));
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(head)
}

fn revert(lander: &Lander, worktree: &Path, request: &Request, tip: &str, lane: &str, cases: &[Failure]) -> Result<String, String> {
    let branch = format!("sheriff-revert-{}", short(lane));
    if !without(worktree, tip, lane)? {
        return Err(format!("{} no longer reverts cleanly onto {}", short(lane), short(tip)));
    }
    git(worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(worktree, &["log", "-1", "--format=%s", lane])?;
    let since = if tip == request.head { String::new() } else { format!(" and still at the tip {}", short(tip)) };
    let message = format!(
        "Revert {subject}\n\nMain went red at {}{since} ({}): {} fail there and pass on main without {}, so the sheriff reverts it through the landing queue.\n\nReverts: {lane}\nSheriff-Reverts: {lane}\nMain-red: {}",
        short(&request.head),
        request.run,
        names(cases),
        short(lane),
        request.run
    );
    git(worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = queue_revert(lander, worktree, &branch)?;
    let owner = lane_ticket(lander, request, tip, lane);
    let reverted = Reverted { branch: owner.as_ref().map_or(lane, |owner| owner.branch.as_str()), commit: lane, revert: &branch, red: &request.head, failing: &names(cases), log: Path::new(&request.log) };
    let told = owner::tell(&owner::Cli, owner.as_ref().and_then(|owner| owner.session.as_deref()), &reverted);
    println!("north land: sheriff: {told}");
    Ok(format!("revert {lane} {head} queued as {branch}"))
}

fn answer(lander: &Lander, worktree: &Path, name: &str, tip: &str) -> Result<String, String> {
    git(worktree, &["fetch", "-q", "--no-tags", "origin", name, "refs/heads/main"])?;
    let main = git(&lander.repo, &["ls-remote", "origin", "refs/heads/main"])?.split_whitespace().next().unwrap_or_default().to_owned();
    let head = git(worktree, &["rev-parse", &format!("{tip}^")])?;
    if name.strip_prefix(REQUESTS) != Some(head.as_str()) {
        return Ok(format!("malformed: {name} does not sit on the commit it names"));
    }
    let request = parse(&head, &git(worktree, &["log", "-1", "--format=%B", tip])?);
    if request.parent.is_empty() {
        return Ok(format!("malformed: {name} names no Green-parent"));
    }
    let lanes = lanes(worktree, &request.parent, &main)?;
    let probes = Probes { lander, worktree, request: &request, tip: &main };
    match find(&request, &main, &lanes, &reverted(lander), |lane| probes.at(lane), |lane| probes.without(lane)) {
        Finding::NoVerdict(reason) => Ok(format!("no verdict: {reason}; nothing is reverted")),
        Finding::Fixed { cases } => Ok(format!("fixed: {} pass at the tip {}; nothing is reverted", names(&cases), short(&main))),
        Finding::Environment { cases } => Ok(format!("environment {} still fail on main without each landed lane; nothing is reverted", names(&cases))),
        Finding::Revert { lane, cases } => revert(lander, worktree, &request, &main, &lane, &cases),
        Finding::Flaky { lane, cases } => {
            let text = format!("north land: sheriff: {} failed at the tip {}, passed on main without {}, then passed at the same tip on a rerun, so they are flaky, not that lane's failure; nothing is reverted; file the flaky test on its owner; log: {}", names(&cases), short(&main), short(&lane), request.log);
            if !super::after::send("proxy", &text) {
                println!("{text}");
            }
            Ok(format!("flaky: {} flipped between runs at the tip {}; nothing is reverted", names(&cases), short(&main)))
        }
    }
}

pub fn requests(lander: &Lander) -> Result<Vec<(String, String)>, String> {
    let listing = git(&lander.repo, &["ls-remote", "origin", &format!("{REQUESTS}*")])?;
    Ok(listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, name)| name.strip_prefix(REQUESTS).is_some_and(|head| !records(lander).join(queue::slug(head)).exists()))
        .map(|(tip, name)| (tip.to_owned(), name.to_owned()))
        .collect())
}

pub fn request_message(head: &str, parent: &str, failing: &BTreeSet<Failure>, log: &Path) -> String {
    let lines: Vec<String> = failing.iter().map(Failure::line).collect();
    format!("Main red at {head}\n\n{}\n\nGreen-parent: {parent}\nMain-red-run: post-land checks of {}\nMain-red-log: {}", lines.join("\n"), short(head), log.display())
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

pub fn file(repo: &Path, head: &str, parent: &str, failing: &BTreeSet<Failure>, log: &Path) -> Result<(), String> {
    let request = git(repo, &["commit-tree", &format!("{head}^{{tree}}"), "-p", head, "-m", &request_message(head, parent, failing, log)])?;
    let pushed = Command::new("git").arg("-C").arg(repo).args(["push", "-q", "--no-verify", "origin", &format!("{request}:{REQUESTS}{head}")]).env("SAFE_PUSH_ACTIVE", "1").output().map_err(|error| error.to_string())?;
    if pushed.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&pushed.stderr).trim().to_owned()) }
}

pub fn chase(lander: &Lander, head: &str, failing: &BTreeSet<Failure>, log: &Path) -> Result<String, String> {
    if records(lander).join(queue::slug(head)).exists() {
        return Ok(format!("main red at {} was already answered", short(head)));
    }
    if failing.is_empty() {
        return Ok(format!("the red post-land run of {} names no failing step, so no check reached a verdict and no revert is requested", short(head)));
    }
    let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let repository = crate::platform::push_ledger::repository(&origin);
    let history = git(&lander.repo, &["log", "--first-parent", "--format=%H", "-n", "400", &format!("{head}^")])?;
    let ledger = fs::read_to_string(post_land::ledger_path()).unwrap_or_default();
    let Some(parent) = green_ancestor(&ledger, &repository, &history.lines().collect::<Vec<_>>()) else {
        return Ok(format!("no landing within 400 of {} has a green post-land verdict, so no revert is requested", short(head)));
    };
    file(&lander.repo, head, &parent, failing, log)?;
    Ok(format!("main was green at {} and is red at {} on {}; the sheriff bisects the landings between", short(&parent), short(head), names(&failing.iter().cloned().collect::<Vec<_>>())))
}

pub fn patrol(lander: &Lander) -> Result<(), String> {
    for (tip, name) in requests(lander)? {
        let (tip, name) = (tip.as_str(), name.as_str());
        let Some(head) = name.strip_prefix(REQUESTS) else { continue };
        let record = records(lander).join(queue::slug(head));
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
