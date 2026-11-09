use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;

use super::candidate::{self, git, output};
use super::queue::rfc3339;
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
    HostFault { host: String, cases: Vec<Failure>, elsewhere: Option<String> },
}

/// Failing checks rerun on one host: the host they ran on and what failed there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Probed {
    pub host: String,
    pub failures: BTreeSet<Failure>,
}

/// Where a probe runs: any host the bench picks, the host this case is pinned to, or any host but that one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum On<'a> {
    Any,
    Host(&'a str),
    Besides(&'a str),
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

fn bisect(case: &mut SheriffCase, failing: &BTreeSet<Failure>, lanes: &[Lane], at: &mut Rerun) -> Option<usize> {
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

type Rerun<'a> = dyn FnMut(&Lane) -> Option<BTreeSet<Failure>> + 'a;

fn reproduce(case: &mut SheriffCase, index: usize, lane: &Lane, newest: &Lane, failing: &BTreeSet<Failure>, at: &mut Rerun, probe: &mut Rerun) -> Result<Option<BTreeSet<Failure>>, String> {
    let mut held = failing.clone();
    for round in 0..2 {
        if round > 0 {
            steps::sheriff(case, &SheriffStep::Confirm(index as u64));
            let base = probe(lane).ok_or_else(|| format!("on main without {} a second time", short(&lane.commit)))?;
            case.confirmations.push((index as u64, Some(labels(&base))));
            if !base.is_disjoint(&held) {
                return Ok(None);
            }
        }
        steps::sheriff(case, &SheriffStep::Recheck);
        let again = at(newest).ok_or_else(|| format!("at the tip {} after the confirmation", short(&newest.commit)))?;
        case.probes.push((0, labels(&again)));
        held = held.intersection(&again).cloned().collect();
        if held.is_empty() {
            return Ok(None);
        }
    }
    Ok(Some(held))
}

/// Bisects and confirms on one host, so a failure only that host produces can never pass for a lane's: Some names the
/// finding, None means the failing checks still fail without every suspect there.
fn on_one_host(case: &mut SheriffCase, cases: &[Failure], failing: &BTreeSet<Failure>, lanes: &[Lane], reverted: &BTreeSet<String>, at: &mut Rerun, probe: &mut Rerun) -> Option<Finding> {
    let newest = &lanes[0];
    let suspects: Vec<usize> = match bisect(case, failing, lanes, at) {
        Some(culprit) => vec![culprit],
        None => (0..lanes.len()).collect(),
    };
    for index in suspects {
        let lane = &lanes[index];
        if is_revert(&lane.message) || reverted.contains(&lane.commit) {
            continue;
        }
        steps::sheriff(case, &SheriffStep::Confirm(index as u64));
        let still = probe(lane);
        case.confirmations.push((index as u64, still.as_ref().map(labels)));
        if !still.is_some_and(|still| still.is_disjoint(failing)) {
            continue;
        }
        return Some(match reproduce(case, index, lane, newest, failing, at, probe) {
            Err(reason) => {
                steps::sheriff(case, &SheriffStep::NoVerdict);
                Finding::NoVerdict(format!("rerunning {} {reason} reached no verdict", names(cases)))
            }
            Ok(None) => {
                steps::sheriff(case, &SheriffStep::Flaky);
                Finding::Flaky { lane: lane.commit.clone(), cases: cases.to_vec() }
            }
            Ok(Some(held)) => {
                steps::sheriff(case, &SheriffStep::Revert(index as u64));
                Finding::Revert { lane: lane.commit.clone(), cases: held.into_iter().collect() }
            }
        });
    }
    None
}

/// The sheriff's finding for a red main. Every probe after the first runs on the host the first ran on, so a revert
/// always rests on the failing checks failing at the tip and passing without the lane on that one host; checks that
/// fail there whatever lane is removed rerun at the tip on another host, and pass there as that host's fault.
pub fn find(
    request: &Request,
    tip: &str,
    lanes: &[Lane],
    reverted: &BTreeSet<String>,
    mut at: impl FnMut(&Lane, On) -> Option<Probed>,
    mut probe: impl FnMut(&Lane, On) -> Option<Probed>,
) -> Finding {
    let mut case = SheriffCase { head: request.head.clone(), tip: tip.to_owned(), failing: labels(&request.failing), lanes: lanes.iter().map(|lane| lane.commit.clone()).collect(), probes: Vec::new(), confirmations: Vec::new(), host: String::new() };
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
    let Some(Probed { host, failures: reproduced }) = at(newest, On::Any) else {
        return no_verdict(&case, format!("rerunning {} at the tip {} reached no verdict", names(&request.failing.iter().cloned().collect::<Vec<_>>()), short(tip)));
    };
    case.host.clone_from(&host);
    case.probes.push((0, labels(&reproduced)));
    let failing: BTreeSet<Failure> = reproduced.intersection(&request.failing).cloned().collect();
    let cases: Vec<Failure> = if failing.is_empty() { request.failing.iter().cloned().collect() } else { failing.iter().cloned().collect() };
    if failing.is_empty() {
        steps::sheriff(&case, &SheriffStep::Fixed);
        return Finding::Fixed { cases };
    }
    let mut at_host = |lane: &Lane| at(lane, On::Host(&host)).map(|probed| probed.failures);
    let mut probe_host = |lane: &Lane| probe(lane, On::Host(&host)).map(|probed| probed.failures);
    if let Some(finding) = on_one_host(&mut case, &cases, &failing, lanes, reverted, &mut at_host, &mut probe_host) {
        return finding;
    }
    steps::sheriff(&case, &SheriffStep::Elsewhere);
    match at(newest, On::Besides(&host)).filter(|other| other.host != host) {
        Some(other) if other.failures.is_disjoint(&failing) => {
            steps::sheriff(&case, &SheriffStep::HostFault);
            Finding::HostFault { host, cases, elsewhere: Some(other.host) }
        }
        Some(_) => {
            steps::sheriff(&case, &SheriffStep::Environment);
            Finding::Environment { cases }
        }
        None => {
            steps::sheriff(&case, &SheriffStep::HostFault);
            Finding::HostFault { host, cases, elsewhere: None }
        }
    }
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

    /// The host a probe runs on: the case's pinned host, else one `north capacity offload choose` picks away from `avoid`;
    /// tests the sheriff runs itself run here unless they must run elsewhere.
    fn place(&self, on: On, tests: bool) -> Option<String> {
        let here = north_base::platform::host::name();
        match on {
            On::Host(host) => Some(host.to_owned()),
            On::Any if tests => Some(here),
            On::Any => post_land::choose_host(&self.lander.repo, None),
            On::Besides(avoid) => post_land::choose_host(&self.lander.repo, Some(avoid)).filter(|host| host != avoid),
        }
    }

    fn rerun(&self, commit: &str, tree: &str, on: On) -> Option<Probed> {
        let tests = self.tests();
        let host = self.place(on, !tests.is_empty())?;
        if tests.is_empty() {
            let steps = self.steps();
            if steps.is_empty() {
                return None;
            }
            println!("north land: sheriff: rerunning {} fresh, never from the verdict cache, on {tree} on {host}", steps.join(", "));
            let failures = post_land::rerun(self.lander, &self.request.parent, commit, &steps, &host)?;
            return Some(Probed { host, failures });
        }
        let command = red_check::rerun_command(&tests)?;
        println!("north land: sheriff: running {} on {tree} on {host}", names(&tests.iter().cloned().collect::<Vec<_>>()));
        let (passed, log) = if host == north_base::platform::host::name() {
            git(self.worktree, &["reset", "-q", "--hard"]).ok()?;
            git(self.worktree, &["checkout", "-q", "--detach", commit]).ok()?;
            let run = Command::new("timeout").args([PROBE_SECONDS, "bash", "-c", &command]).current_dir(self.worktree).output().ok()?;
            (run.status.success(), format!("{}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr)))
        } else {
            let run = Command::new(super::after::follow::program())
                .args(["capacity", "offload", "run", "--host", &host, "--base", commit, "--head", commit, "--timeout", PROBE_SECONDS, "--", &command])
                .current_dir(&self.lander.repo)
                .env(north_base::foundation::offload::OWNER_ENV, &self.lander.offload_owner)
                .stdin(std::process::Stdio::null())
                .output()
                .ok()?;
            (run.status.success(), format!("{}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr)))
        };
        let failures = red_check::failures(&log);
        (passed || !failures.is_empty()).then_some(Probed { host, failures })
    }

    fn at(&self, lane: &Lane, on: On) -> Option<Probed> {
        self.rerun(&lane.commit, &format!("main at {}", short(&lane.commit)), on)
    }

    fn without(&self, lane: &Lane, on: On) -> Option<Probed> {
        if !without(self.worktree, self.tip, &lane.commit).ok()? {
            return None;
        }
        let message = format!("sheriff probe: main {} without {}", short(self.tip), short(&lane.commit));
        git(self.worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message]).ok()?;
        let probe = git(self.worktree, &["rev-parse", "HEAD"]).ok()?;
        self.rerun(&probe, &format!("main {} without {}", short(self.tip), short(&lane.commit)), on)
    }
}

fn lane_ticket(lander: &Lander, request: &Request, tip: &str, lane: &str) -> Option<Ticket> {
    let done = fs::read_dir(lander.queue.join("done")).ok()?;
    let store = north_base::land_ticket::recorded(&lander.queue);
    let landed = done.filter_map(Result::ok).map(|entry| entry.path()).filter_map(|path| Some((queue::read(&path).ok()?, path)));
    landed.filter(|(ticket, path)| north_base::land_ticket::read(store.as_ref(), &lander.queue, path, ticket.verdict.as_deref()) == north_base::land_ticket::Phase::Landed).map(|(ticket, _)| ticket).find(|ticket| {
        ticket.head == lane || git(&lander.repo, &["cherry", &ticket.head, tip, &request.parent]).is_ok_and(|cherry| cherry.lines().any(|line| line == format!("- {lane}")))
    })
}

pub fn queue_revert(lander: &Lander, worktree: &Path, branch: &str) -> Result<String, String> {
    queue_revert_in(&lander.repo, &lander.queue, worktree, branch)
}

pub fn queue_revert_in(repo: &Path, queue_dir: &Path, worktree: &Path, branch: &str) -> Result<String, String> {
    let head = git(worktree, &["rev-parse", "HEAD"])?;
    let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned();
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| error.to_string())?.as_micros();
    let ticket = Ticket { origin, root: worktree.to_owned(), host, branch: branch.to_owned(), head: head.clone(), session: None, queued_at: rfc3339(OffsetDateTime::now_utc()), queued_micros: Some(micros), first_commit_secs: None, several_commits: None, red_fix: Some(RedFix::Marker), verdict: None, deferral: None, keep_lane: true, urgent: None, urgent_at: None, lander_fix: None, decided_on: None, published: None, alone_green: None, alone_base: None };
    let path = queue_dir.join(queue::file_name(micros, branch));
    queue::write(&path, &ticket).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    north_base::land_ticket::queued(&path, "the sheriff queued a revert").map_err(|error| format!("cannot record {}: {error}", path.display()))?;
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
        "Revert {subject}\n\nMain went red at {}{since} ({}): {} fail there twice and pass twice on main without {} in back-to-back reruns, so the sheriff reverts it through the landing queue.\n\nReverts: {lane}\nSheriff-Reverts: {lane}\nMain-red: {}",
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

/// A failure that fails at the tip and on main without each landed lane, on every host: main itself is broken, so no lane is
/// reverted and the proxy is told which checks to fix.
fn main_breakage(request: &Request, tip: &str, cases: &[Failure]) -> String {
    let text = format!("north land: sheriff: main is broken, not a lane: {} fail on a fresh run at main {} and on main without each landed lane since {}; nothing is reverted; log: {}", names(cases), short(tip), short(&request.parent), request.log);
    if !super::after::send("proxy", &text) {
        println!("{text}");
    }
    format!("main breakage: {} fail on fresh runs at the tip {} and on main without each landed lane; the proxy is told; nothing is reverted", names(cases), short(tip))
}

/// A failure the pinned host gives at every commit: no lane caused it, so nothing is reverted; when no other host reached a
/// verdict either, the proxy is paged, since no host can say whether main is red.
fn host_fault(request: &Request, tip: &str, host: &str, cases: &[Failure], elsewhere: Option<&str>) -> String {
    let Some(other) = elsewhere else {
        let text = format!("north land: sheriff: {} fail on {host} at main {} and on main without each landed lane, and no other host reached a verdict on them, so no host can say whether main is red; nothing is reverted; log: {}", names(cases), short(tip), request.log);
        if !super::after::send("proxy", &text) {
            println!("{text}");
        }
        return format!("host fault on {host}: {} fail there whatever lane is removed and no other host reached a verdict; the proxy is paged; nothing is reverted", names(cases));
    };
    format!("host fault on {host}: {} fail there whatever lane is removed and pass at the tip {} on {other}, so {host}, not main, is at fault; nothing is reverted", names(cases), short(tip))
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
    if let Some(verdict) = north_check_environment::credential::incident(Path::new(&request.log)) {
        return Ok(verdict);
    }
    let lanes = lanes(worktree, &request.parent, &main)?;
    let probes = Probes { lander, worktree, request: &request, tip: &main };
    match find(&request, &main, &lanes, &reverted(lander), |lane, on| probes.at(lane, on), |lane, on| probes.without(lane, on)) {
        Finding::NoVerdict(reason) => Ok(format!("no verdict: {reason}; nothing is reverted")),
        Finding::Fixed { cases } => Ok(format!("fixed: {} pass at the tip {}; nothing is reverted", names(&cases), short(&main))),
        Finding::Environment { cases } => Ok(main_breakage(&request, &main, &cases)),
        Finding::HostFault { host, cases, elsewhere } => Ok(host_fault(&request, &main, &host, &cases, elsewhere.as_deref())),
        Finding::Revert { lane, cases } => revert(lander, worktree, &request, &main, &lane, &cases),
        Finding::Flaky { lane, cases } => {
            let text = format!("north land: sheriff: {} failed at the tip {} and passed on main without {}, but did not fail twice at the tip and pass twice without it in back-to-back reruns, so they are flaky, not that lane's failure; nothing is reverted; file the flaky test on its owner; log: {}", names(&cases), short(&main), short(&lane), request.log);
            if !super::after::send("proxy", &text) {
                println!("{text}");
            }
            Ok(format!("flaky: {} flipped between runs at the tip {}; nothing is reverted", names(&cases), short(&main)))
        }
    }
}

fn requests(lander: &Lander) -> Result<Vec<(String, String)>, String> {
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
