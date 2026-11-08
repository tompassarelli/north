use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::landing::red_check::Failure;
use crate::platform::push_ledger;
use crate::landing::check_timings::Deadline;
use super::after::{follow, send};
use super::{blame, probe_slots};
use super::candidate::git;
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::setting;
use super::telemetry;

const ALARMED: &str = "proxy";
const POLL: Duration = Duration::from_millis(50);
const EPISODE: &str = "post-land-red";
const DEADLINE_SETTING: &str = "landing.after-deadline-sec";

mod execute;
mod runner;
mod settle;
pub use runner::{Record, SLICE_ENV, abandon, adopt, adoptable, after_owner, reap_scratch};
pub(crate) use execute::{Checked, failing, steps};
use execute::{check, judged, record};
pub(crate) use settle::rerun;
pub use settle::settle;
use settle::{offload_rerun, settle_from};

#[derive(Clone)]
pub struct Landed {
    pub repo: PathBuf,
    pub repository: String,
    pub base: String,
    pub head: String,
    pub heads: Vec<String>,
    pub lanes: Vec<String>,
    pub tips: Vec<String>,
}

pub struct Outcome {
    pub verdict: Verdict,
    pub search: Option<Search>,
    pub log: PathBuf,
    pub failing: BTreeSet<Failure>,
    pub superseded: bool,
}

pub struct Search {
    landed: Landed,
    run: Run,
}

pub struct Found {
    pub culprit: Option<usize>,
    pub span: (f64, f64),
    pub superseded: bool,
}

pub struct Run {
    pub runner: Vec<String>,
    pub worktree: PathBuf,
    pub log: PathBuf,
    pub outcome: PathBuf,
    pub ledger: PathBuf,
    pub store: PathBuf,
    pub offload_owner: String,
    pub capacity: String,
    pub deadline: Duration,
    pub host: Option<String>,
    pub cancel: Arc<AtomicBool>,
    pub adoptable: bool,
}

#[derive(Default)]
pub struct Hosts(Vec<String>);

impl Hosts {
    pub fn claim(&mut self, repo: &Path, head: &str) -> Option<String> {
        let taken = &mut self.0;
        let mut command = Command::new(follow::program());
        command.args(["capacity", "offload", "choose", "--any"]);
        for host in taken.iter() {
            command.args(["--taken", host]);
        }
        let chosen = command.current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output();
        let host = chosen.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|host| !host.is_empty());
        match &host {
            Some(host) if taken.is_empty() => println!("north land: the after-landing run of {head} runs its checks on {host}"),
            Some(host) => println!("north land: the after-landing run of {head} runs its checks on {host}, beside the runs in flight on {}", taken.join(", ")),
            None => {}
        }
        taken.extend(host.clone());
        host
    }

    pub fn hold(&mut self, host: &str) {
        self.0.push(host.to_owned());
    }

    pub fn release(&mut self, host: &str) {
        if let Some(at) = self.0.iter().position(|held| held == host) {
            self.0.remove(at);
        }
    }
}

/// The host `north capacity offload choose --any` gives a check, away from `avoid`: a build host by name, else this host
/// when it has the most room; None when no host answers.
pub fn choose_host(repo: &Path, avoid: Option<&str>) -> Option<String> {
    let mut command = Command::new(follow::program());
    command.args(["capacity", "offload", "choose", "--any"]);
    if let Some(avoid) = avoid {
        command.args(["--avoid", avoid]);
    }
    let chosen = command.current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output().ok().filter(|output| output.status.success())?;
    let host = String::from_utf8_lossy(&chosen.stdout).trim().to_owned();
    Some(if host.is_empty() { north_base::platform::host::name() } else { host })
}

impl Run {
    fn at(&self, head: &str, tree: &str) -> Self {
        let short = |sha: &str| sha[..sha.len().min(12)].to_owned();
        let name = if tree == head { format!("post-land-{}", short(head)) } else { format!("post-land-{}-on-{}", short(head), short(tree)) };
        let beside = |path: &Path, file: String| path.with_file_name(file);
        Self {
            runner: self.runner.clone(),
            worktree: beside(&self.worktree, name.clone()),
            log: beside(&self.log, format!("{name}.log")),
            outcome: beside(&self.outcome, format!("{name}.outcome")),
            ledger: self.ledger.clone(),
            store: self.store.clone(),
            offload_owner: self.offload_owner.clone(),
            capacity: self.capacity.clone(),
            deadline: self.deadline,
            host: self.host.clone(),
            cancel: Arc::clone(&self.cancel),
            adoptable: self.adoptable && tree == head,
        }
    }
}

impl Run {
    fn superseded(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn superseded(landed: &Landed) -> Verdict {
    Verdict::Transient(format!("a later landing changed the check lines of the after-landing run of {}, so it was cancelled as superseded and reached no verdict", landed.head))
}

pub fn ledger_path() -> PathBuf {
    state_home().join("safe-push").join("post-land.tsv")
}

pub(crate) fn deadline(repo: &Path, ledger: &Path, repository: &str, head: &str) -> Duration {
    let set = setting::landing(repo, "refs/remotes/origin/main", DEADLINE_SETTING).trim().parse().ok().filter(|seconds| *seconds > 0);
    let seconds = set.unwrap_or_else(|| {
        let deadline = measured_deadline(ledger, repository);
        println!("north land: the after-landing run of {head} has a {deadline}");
        deadline.seconds
    });
    Duration::from_secs(seconds)
}

fn measured_deadline(ledger: &Path, repository: &str) -> Deadline {
    let text = fs::read_to_string(ledger).unwrap_or_default();
    let runs: Vec<(&str, u64)> = text.lines().map(|row| row.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&repository) && fields.get(7).is_none_or(|covering| covering.is_empty())).filter_map(|fields| Some((*fields.get(3)?, fields.get(6)?.parse().ok()?))).collect();
    let walls: Vec<u64> = runs.iter().filter(|(class, _)| matches!(*class, "green" | "red-main")).map(|(_, seconds)| *seconds).collect();
    let exceeded = runs.last().filter(|(class, _)| *class == "transient").map(|(_, seconds)| *seconds);
    Deadline::after_landing(&walls, exceeded)
}

impl Landed {
    pub fn on(lander: &Lander, base: &str, head: &str, heads: Vec<String>, lanes: Vec<String>, tips: Vec<String>) -> Self {
        let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
        Self { repo: lander.repo.clone(), repository: push_ledger::repository(&origin), base: base.to_owned(), head: head.to_owned(), heads, lanes, tips }
    }
}

fn after_run(lander: &Lander, landed: &Landed, host: Option<String>, cancel: Arc<AtomicBool>, deadline: Duration) -> Run {
    let head = landed.head.as_str();
    Run {
        runner: vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--dry".to_owned(), "1".to_owned()],
        worktree: lander.queue.join("post-land"),
        log: lander.queue.join("logs").join("post-land"),
        outcome: lander.queue.join("logs").join("post-land"),
        ledger: ledger_path(),
        store: north_base::lifecycle::Store::host_path().unwrap_or_default(),
        offload_owner: after_owner(&lander.offload_owner),
        capacity: setting::landing(&lander.repo, "refs/remotes/origin/main", "landing.capacity"),
        deadline,
        host: host.or_else(|| (setting::landing(&lander.repo, "refs/remotes/origin/main", "land.runner") == "build-host").then(north_base::platform::host::name)),
        cancel,
        adoptable: true,
    }
    .at(head, head)
}

pub fn start(lander: &Lander, landed: Landed, host: Option<String>, cancel: Arc<AtomicBool>) -> std::thread::JoinHandle<Outcome> {
    let run = after_run(lander, &landed, host, cancel, deadline(&lander.repo, &ledger_path(), &landed.repository, &landed.head));
    std::thread::spawn(move || {
        let verdict = settle(&landed, &run, send);
        outcome(landed, run, verdict)
    })
}

fn outcome(landed: Landed, run: Run, verdict: Verdict) -> Outcome {
    println!("north land: post-land checks of {} ended {verdict:?}", landed.head);
    if !matches!(verdict, Verdict::RedMain(_)) {
        return Outcome { verdict, search: None, failing: BTreeSet::new(), superseded: run.superseded(), log: run.log };
    }
    let failing = failing(&run.log, &run.outcome);
    let (superseded, log) = (run.superseded(), run.log.clone());
    Outcome { verdict, search: Some(Search { landed, run }), failing, log, superseded }
}

pub fn search(search: Search) -> std::thread::JoinHandle<Found> {
    std::thread::spawn(move || {
        let Search { landed, run } = search;
        let started = telemetry::epoch();
        let culprit = locate(&landed, &run);
        if run.superseded() {
            println!("north land: the culprit search of {} stopped because a later landing changed its check lines", landed.head);
            return Found { culprit: None, span: (started, telemetry::epoch()), superseded: true };
        }
        if let Some(at) = culprit {
            judge_coverage(&landed, if at == 0 { &landed.base } else { &landed.tips[at - 1] }, &landed.tips[at], &run.at(&landed.head, &landed.tips[at]).log);
        }
        Found { culprit, span: (started, telemetry::epoch()), superseded: false }
    })
}


fn judge_coverage(landed: &Landed, base: &str, tip: &str, log: &Path) {
    let judged = Command::new(follow::program()).args(["test", "coverage", "miss", base, tip, "--log"]).arg(log).current_dir(&landed.repo).stdin(Stdio::null()).output();
    match judged {
        Ok(output) => print!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        Err(error) => eprintln!("north land: north test coverage miss: {error}"),
    }
}

fn locate(landed: &Landed, run: &Run) -> Option<usize> {
    let lanes = landed.tips.len();
    let room = probe_slots::free_slots(&landed.repo).map_or(1, |(free, _)| free);
    if room < lanes {
        println!("north land: {room} offload slots are free for the {lanes} prefixes of {}, so the culprit is bisected", landed.head);
    }
    let culprit = blame::culprit(lanes, room, |counts| prefixes(landed, run, counts));
    match culprit {
        Some(at) => println!("north land: {} is the first landed lane whose prefix of {} is red", landed.lanes.get(at).map_or("?", String::as_str), landed.head),
        None => println!("north land: post-land checks of {} are red, but the prefix search names no lane: main was already red at {} or a prefix reached no verdict", landed.head, landed.base),
    }
    culprit
}

fn prefixes(landed: &Landed, run: &Run, counts: &[usize]) -> Vec<Verdict> {
    let tree = |count: usize| if count == 0 { landed.base.as_str() } else { landed.tips[count - 1].as_str() };
    let trees: std::collections::BTreeSet<&str> = counts.iter().map(|&count| tree(count)).collect();
    let verdicts: std::collections::BTreeMap<&str, Verdict> = std::thread::scope(|scope| {
        let checks: Vec<_> = trees.into_iter().map(|at| (at, scope.spawn(move || check(landed, at, &run.at(&landed.head, at)).verdict))).collect();
        checks.into_iter().map(|(at, handle)| (at, handle.join().unwrap_or_else(|_| Verdict::Transient("a prefix check panicked".to_owned())))).collect()
    });
    counts
        .iter()
        .map(|&count| {
            let verdict = verdicts[tree(count)].clone();
            println!("north land: post-land checks with the first {count} of {} landed lanes ended {verdict:?}", landed.tips.len());
            verdict
        })
        .collect()
}


#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
