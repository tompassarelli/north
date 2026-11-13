use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::landing::red_check::Failure;
use crate::platform::push_ledger;
use crate::landing::check_timings::Deadline;
use super::after::{follow, send};
use super::blame;
use super::probe_slots;
use super::candidate::git;
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::setting;
use super::telemetry;
use super::wake::{Task, Wake};

const ALARMED: &str = "proxy";
const POLL: Duration = Duration::from_millis(50);
const EPISODE: &str = "post-land-red";
const DEADLINE_SETTING: &str = "landing.after-deadline-sec";

mod execute;
mod probe;
mod runner;
mod settle;
pub use runner::{Record, abandon, adopt, adoptable, after_owner, reap_scratch};
pub(crate) use execute::{Checked, failing, steps};
use execute::{check, judged, record};
use probe::Probe;
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
    pub ran: Vec<String>,
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
    failing: BTreeSet<Failure>,
}

pub struct Found {
    pub culprits: Vec<usize>,
    pub span: (f64, f64),
    pub superseded: bool,
    pub cancelled_by: Option<String>,
    pub unreproduced: Option<PathBuf>,
}

/// An after-landing run's cancel record: set once, by the lander that supersedes the run, naming the landing whose
/// check lines replaced its own, so the run's verdict, the lander's log and its journaled move say who cancelled it
/// rather than only that a later landing did (north#1247). A run no lander has cancelled holds no sender, and so does
/// one adopted from a lander older than this record.
#[derive(Clone, Default)]
pub struct Cancel(Arc<OnceLock<String>>);

impl Cancel {
    /// Cancels the run on behalf of `sender`: the landing whose check lines made this run's stale.
    pub fn request(&self, sender: &str) {
        let _ = self.0.set(sender.to_owned());
    }

    pub fn requested(&self) -> bool {
        self.0.get().is_some()
    }

    /// The landing that cancelled the run; None while none has, or when the lander that did recorded no name.
    pub fn sender(&self) -> Option<&str> {
        self.0.get().map(String::as_str)
    }
}

#[derive(Clone)]
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
    pub cancel: Cancel,
    pub adoptable: bool,
}

#[derive(Default)]
pub struct Hosts(Vec<String>);

impl Hosts {
    /// The host the after-landing run of `head` checks on, counted as taken until released; None runs it here, as for a repository outside home, which no build host keeps: on 2026-11-10 the runtime canary's scratch landing under /tmp offered its check to cpx62, cpx62b and gex45, each refused it with exit 75 and `land serve --once` retried it for ever.
    pub fn claim(&mut self, repo: &Path, head: &str) -> Option<String> {
        if std::env::var_os("HOME").is_some_and(|home| !repo.starts_with(home)) {
            println!("north land: the after-landing run of {head} runs its checks here, since no build host keeps {} outside home", repo.display());
            return None;
        }
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
            None => println!("north land: no build host answered, so the after-landing run of {head} runs its checks here"),
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
        let name = if tree == head { format!("post-land-{}", short(head)) } else { format!("post-land-{}-on-{}", short(head), short(tree)) };
        self.named(name, tree == head)
    }

    fn again(&self, head: &str) -> Self {
        self.named(format!("post-land-{}-again", short(head)), false)
    }

    fn named(&self, name: String, adoptable: bool) -> Self {
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
            cancel: self.cancel.clone(),
            adoptable: self.adoptable && adoptable,
        }
    }
}

fn short(sha: &str) -> String {
    sha[..sha.len().min(12)].to_owned()
}

impl Run {
    fn superseded(&self) -> bool {
        self.cancel.requested()
    }
}

fn cancelled_by(run: &Run) -> &str {
    run.cancel.sender().unwrap_or("a later landing whose name the lander did not record")
}

fn superseded(landed: &Landed, run: &Run) -> Verdict {
    Verdict::Transient(format!("{} changed the check lines of the after-landing run of {}, so the lander cancelled it as superseded and it reached no verdict", cancelled_by(run), landed.head))
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
    pub fn on(lander: &Lander, base: &str, head: &str, heads: Vec<String>, lanes: Vec<String>, tips: Vec<String>, ran: Vec<String>) -> Self {
        let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
        Self { repo: lander.repo.clone(), repository: push_ledger::repository(&origin), base: base.to_owned(), head: head.to_owned(), heads, lanes, tips, ran }
    }
}

fn after_run(lander: &Lander, landed: &Landed, host: Option<String>, cancel: Cancel, deadline: Duration) -> Run {
    let head = landed.head.as_str();
    Run {
        runner: vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--fresh".to_owned(), "--dry".to_owned(), "1".to_owned()],
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

pub fn start(lander: &Lander, landed: Landed, host: Option<String>, cancel: Cancel) -> Task<Outcome> {
    let run = after_run(lander, &landed, host, cancel, deadline(&lander.repo, &ledger_path(), &landed.repository, &landed.head));
    Task::spawn(&lander.wake, move || {
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
    super::buck2_after::cargo_red(&landed.head, &run.log);
    let (superseded, log) = (run.superseded(), run.log.clone());
    Outcome { verdict, search: Some(Search { landed, run, failing: failing.clone() }), failing, log, superseded }
}

pub fn search(wake: &Arc<Wake>, search: Search) -> Task<Found> {
    Task::spawn(wake, move || {
        let Search { landed, run, failing } = search;
        let started = telemetry::epoch();
        let reproduced = at_the_tip(&landed, &run, &failing);
        let stopped = |unreproduced| Found { culprits: Vec::new(), span: (started, telemetry::epoch()), superseded: run.superseded(), cancelled_by: run.cancel.sender().map(str::to_owned), unreproduced };
        if run.superseded() {
            println!("north land: the culprit search of {} stopped because {} changed its check lines", landed.head, cancelled_by(&run));
            return stopped(None);
        }
        let failing = match reproduced {
            Ok(failing) => failing,
            Err(again) => return stopped(Some(again)),
        };
        let culprits = locate(&landed, &run, &failing);
        if run.superseded() {
            println!("north land: the culprit search of {} stopped because {} changed its check lines", landed.head, cancelled_by(&run));
            return stopped(None);
        }
        for &at in &culprits {
            let probed = run.at(&landed.head, &landed.tips[at]).log;
            judge_coverage(&landed, if at == 0 { &landed.base } else { &landed.tips[at - 1] }, &landed.tips[at], if probed.exists() { &probed } else { &run.log });
        }
        Found { culprits, span: (started, telemetry::epoch()), superseded: false, cancelled_by: None, unreproduced: None }
    })
}

fn at_the_tip(landed: &Landed, run: &Run, failing: &BTreeSet<Failure>) -> Result<BTreeSet<Failure>, PathBuf> {
    let again = run.again(&landed.head);
    let probe = Probe::of(failing);
    let rerun = probe::probe(landed, &again, &landed.head, &probe);
    if !matches!(rerun.verdict, Verdict::RedMain(_)) {
        println!("north land: the failed checks of {} ended {:?} when rerun at the tip, so no lane is blamed", landed.head, rerun.verdict);
        return Err(again.log);
    }
    let reproduced: BTreeSet<Failure> = failing.iter().filter(|failure| !probe.runs(failure) || rerun.failing.contains(failure.label())).cloned().collect();
    if failing.iter().any(|failure| probe.runs(failure)) && !reproduced.iter().any(|failure| probe.runs(failure)) {
        println!("north land: none of the failed checks of {} failed again when rerun at the tip, so no lane is blamed", landed.head);
        return Err(again.log);
    }
    Ok(reproduced)
}


fn judge_coverage(landed: &Landed, base: &str, tip: &str, log: &Path) {
    let judged = Command::new(follow::program()).args(["test", "coverage", "miss", base, tip, "--log"]).arg(log).current_dir(&landed.repo).stdin(Stdio::null()).output();
    match judged {
        Ok(output) => print!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        Err(error) => eprintln!("north land: north test coverage miss: {error}"),
    }
}

const WALK_BACK: usize = 16;
const WALK_BACK_GATE: &str = "blame-walk-back";

fn locate(landed: &Landed, run: &Run, failing: &BTreeSet<Failure>) -> Vec<usize> {
    let lanes = landed.tips.len();
    let room = probe_slots::free_slots(&landed.repo).map_or(1, |(free, _)| free);
    let suspects = blame::ask(&landed.repo, &run.log, &landed.base, &landed.head, &landed.lanes, &landed.tips, room);
    let probe = Probe::of(failing);
    let check = |tree: &str| probe::probe(landed, &run.at(&landed.head, tree), tree, &probe);
    let signatures: BTreeSet<String> = failing.iter().map(|failure| failure.label().to_owned()).collect();
    let searchable = signatures.len() > 1 && !matches!(&suspects, blame::Suspects::Named(named) if named.len() == 1);
    let gates = state_home().join("north/gates");
    let earlier = || git(&landed.repo, &["rev-list", "--first-parent", &format!("--max-count={WALK_BACK}"), &format!("{}^", landed.base)]).map(|listed| listed.lines().map(str::to_owned).collect::<Vec<String>>()).unwrap_or_default();
    let found = if searchable {
        let base = check(&landed.base);
        if !matches!(&base.verdict, Verdict::Green) {
            println!("north land: post-land checks of {} are red on their base {} on the failed tests, so no landed lane is blamed", landed.head, landed.base);
            if matches!(&base.verdict, Verdict::RedMain(_)) {
                walked_back(landed, room, &check, &earlier(), &gates, failing);
            }
            return Vec::new();
        }
        blame::culprits(lanes, &suspects, &signatures, |counts| probed(landed, counts, &check))
    } else {
        None
    };
    let culprits: Vec<usize> = match found {
        Some(found) if !found.is_empty() => found,
        _ => blamed(landed, room, &suspects, &check, earlier, &gates, failing).into_iter().collect(),
    };
    for &at in &culprits {
        println!("north land: {} is a landed lane whose prefix of {} is red", landed.lanes.get(at).map_or("?", String::as_str), landed.head);
    }
    if culprits.is_empty() {
        println!("north land: post-land checks of {} are red, but the prefix search names no lane: main was already red at {} or a prefix reached no verdict", landed.head, landed.base);
    }
    culprits
}

/// Blames the first lane of `landed` red on the failed checks where its parent passes; a base red on them walks back
/// over the landings before it, in shadow: the earlier lane it would revert goes to the `blame-walk-back` gate log.
pub(super) fn blamed(landed: &Landed, room: usize, suspects: &blame::Suspects, check: &(dyn Fn(&str) -> blame::Probe + Sync), earlier: impl Fn() -> Vec<String>, gates: &Path, failing: &BTreeSet<Failure>) -> Option<usize> {
    let blame = blame::culprit(landed.tips.len(), room, suspects, |counts| probed(landed, counts, check).into_iter().map(|probe| probe.verdict).collect());
    match blame {
        blame::Blame::Lane(at) => {
            println!("north land: {} is the first landed lane of {} red on the failed checks where its parent passes them", landed.lanes.get(at).map_or("?", String::as_str), landed.head);
            Some(at)
        }
        blame::Blame::BaseRed => {
            walked_back(landed, room, check, &earlier(), gates, failing);
            None
        }
        blame::Blame::Unknown => {
            println!("north land: post-land checks of {} are red, but the prefix search reached no verdict", landed.head);
            None
        }
    }
}

fn walked_back(landed: &Landed, room: usize, check: &(dyn Fn(&str) -> blame::Probe + Sync), earlier: &[String], gates: &Path, failing: &BTreeSet<Failure>) {
    let tree = |at: usize| if at == 0 { landed.base.as_str() } else { earlier[at - 1].as_str() };
    let found = blame::walk_back(earlier.len(), room, |ats| checked(&ats.iter().map(|&at| tree(at)).collect::<Vec<_>>(), check).into_iter().map(|probe| probe.verdict).collect()).map(tree);
    match found {
        Some(commit) => println!("north land: main was already red at {} on the failed checks; walking back, {commit} is the first landing red where its parent passes, and in shadow it is logged, not reverted", landed.base),
        None => println!("north land: main was already red at {} on the failed checks, and walking back over {} earlier landings found none red where its parent passes", landed.base, earlier.len()),
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": WALK_BACK_GATE, "at": at, "head": landed.head, "base": landed.base, "would_revert": found, "failing": failing.iter().map(Failure::label).collect::<Vec<_>>()});
    let _ = fs::create_dir_all(gates);
    if let Err(error) = fs::OpenOptions::new().create(true).append(true).open(gates.join(format!("{WALK_BACK_GATE}.jsonl"))).and_then(|mut file| std::io::Write::write_all(&mut file, format!("{line}\n").as_bytes())) {
        eprintln!("north land: could not log the blame walk-back of {}: {error}", landed.head);
    }
}

fn probed(landed: &Landed, counts: &[usize], check: &(dyn Fn(&str) -> blame::Probe + Sync)) -> Vec<blame::Probe> {
    let tree = |count: usize| if count == 0 { landed.base.as_str() } else { landed.tips[count - 1].as_str() };
    let probes = checked(&counts.iter().map(|&count| tree(count)).collect::<Vec<_>>(), check);
    for (count, probe) in counts.iter().zip(&probes) {
        println!("north land: the failed checks with the first {count} of {} landed lanes ended {:?}", landed.tips.len(), probe.verdict);
    }
    probes
}

fn checked(trees: &[&str], check: &(dyn Fn(&str) -> blame::Probe + Sync)) -> Vec<blame::Probe> {
    let distinct: BTreeSet<&str> = trees.iter().copied().collect();
    let probes: std::collections::BTreeMap<&str, blame::Probe> = std::thread::scope(|scope| {
        let checks: Vec<_> = distinct.into_iter().map(|tree| (tree, scope.spawn(move || check(tree)))).collect();
        checks.into_iter().map(|(tree, handle)| (tree, handle.join().unwrap_or_else(|_| blame::Probe { verdict: Verdict::Transient("a prefix check panicked".to_owned()), failing: BTreeSet::new() }))).collect()
    });
    trees.iter().map(|tree| probes[*tree].clone()).collect()
}


#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
