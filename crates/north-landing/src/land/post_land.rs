use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::landing::red_check::Failure;
use north_base::platform::push_ledger;
use crate::landing::check_timings::Deadline;
use super::after::{follow, send};
use super::candidate::git;
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::setting;
use super::telemetry;
use super::wake::{Task, Wake};

const ALARMED: &str = "coordinator";
const POLL: Duration = Duration::from_millis(50);
const EPISODE: &str = "post-land-red";
const CLEARED: &str = "post-land-cleared";
const DEADLINE_SETTING: &str = "landing.after-deadline-sec";

mod execute;
mod probe;
mod resume;
mod search;
mod runner;
mod settle;
pub use resume::resume;
pub use runner::{Record, abandon, adopt, adoptable, after_owner, reap_scratch};
pub(crate) use execute::{Checked, failing};
use execute::{apart, check, judged, record};
use probe::Probe;
pub use settle::{covered, settle};
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

impl Search {
    pub fn limit(&self) -> Duration {
        let rounds = SEARCH_PASSES_PER_LANE * self.landed.tips.len() + SEARCH_FIXED_ROUNDS;
        self.run.deadline * u32::try_from(rounds).unwrap_or(u32::MAX)
    }
}

const SEARCH_PASSES_PER_LANE: usize = 2;
const SEARCH_FIXED_ROUNDS: usize = 2;

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

pub struct Unanswered;

#[derive(Default)]
pub struct Hosts(Vec<String>);

impl Hosts {
    /// The build host the after-landing run of `head` checks on, other than this host and counted as taken until released; None runs it here, as for a repository outside home, which no build host keeps: on 2026-11-10 the runtime canary's scratch landing under /tmp offered its check to cpx62, cpx62b and gex45, each refused it with exit 75 and `land serve --once` retried it for ever. Unanswered when no other build host answers, so the run waits rather than compile on this host (2026-11-11); under a test fixture (NORTH_TEST_ROOT set) it runs here instead.
    pub fn claim(&mut self, repo: &Path, head: &str) -> Result<Option<String>, Unanswered> {
        if std::env::var_os("HOME").is_some_and(|home| !repo.starts_with(home)) {
            println!("north land: the after-landing run of {head} runs its checks here, since no build host keeps {} outside home", repo.display());
            return Ok(None);
        }
        let taken = &mut self.0;
        let settling = north_base::platform::host::name();
        let mut command = Command::new(follow::program());
        command.args(["capacity", "offload", "choose", "--any"]).args(["--avoid", settling.as_str()]);
        for host in taken.iter() {
            command.args(["--taken", host]);
        }
        let chosen = command.current_dir(repo).stdin(Stdio::null()).stderr(Stdio::null()).output();
        let host = chosen.ok().filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|host| !host.is_empty());
        let Some(host) = host else {
            if north_base::platform::land_queue::test_root().is_some() {
                println!("north land: no build host answered the after-landing run of {head}, so the test fixture runs its checks here");
                return Ok(None);
            }
            return Err(Unanswered);
        };
        if taken.is_empty() {
            println!("north land: the after-landing run of {head} runs its checks on {host}");
        } else {
            println!("north land: the after-landing run of {head} runs its checks on {host}, beside the runs in flight on {}", taken.join(", "));
        }
        taken.push(host.clone());
        Ok(Some(host))
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

/// The one host a rerun runs on and its command is composed for: the run's own host, else the one `choose_host` gives, which
/// falls back to this host when no build host answers; None when that is this host and it is closed to offloaded checks.
pub(super) fn rerun_host(repo: &Path, run: &Run) -> Option<String> {
    let host = match &run.host {
        Some(host) => host.clone(),
        None => choose_host(repo, None)?,
    };
    let closed_here = Command::new(follow::program()).args(["capacity", "offload", "closed"]).current_dir(repo).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.code() == Some(75));
    (!closed_here || host != north_base::platform::host::name()).then_some(host)
}

impl Run {
    fn at(&self, head: &str, tree: &str) -> Self {
        let name = if tree == head { format!("post-land-{}", short(head)) } else { format!("post-land-{}-on-{}", short(head), short(tree)) };
        self.named(name, tree == head)
    }

    fn again(&self, head: &str) -> Self {
        self.named(format!("post-land-{}-again", short(head)), false)
    }

    /// The run as its culprit search reruns it: under the yielding offload owner, so its probes take only cores sessions and hand-in checks leave idle.
    fn yielding(self) -> Self {
        Self { offload_owner: north_foundation::foundation::offload::yielding_owner(&self.offload_owner), ..self }
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
        let origin = git(&lander.worktree, &["remote", "get-url", "origin"]).unwrap_or_default();
        Self { repo: lander.worktree.clone(), repository: push_ledger::repository(&origin), base: base.to_owned(), head: head.to_owned(), heads, lanes, tips, ran }
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

pub fn limit(lander: &Lander, landed: &Landed) -> Duration {
    deadline(&lander.repo, &ledger_path(), &landed.repository, &landed.head)
}

pub fn start(lander: &Lander, landed: Landed, host: Option<String>, cancel: Cancel, limit: Duration) -> Task<Outcome> {
    let run = after_run(lander, &landed, host, cancel, limit);
    Task::spawn(&lander.wake, move || {
        let known = settle::known_failing(&landed, &run);
        let verdict = settle(&landed, &run, send);
        outcome(landed, run, verdict, &known)
    })
}

fn already_known(failing: &BTreeSet<Failure>, known: &BTreeSet<Failure>) -> bool {
    !failing.is_empty() && failing.is_subset(known)
}

fn outcome(landed: Landed, run: Run, verdict: Verdict, known: &BTreeSet<Failure>) -> Outcome {
    println!("north land: post-land checks of {} ended {verdict:?}", landed.head);
    if matches!(verdict, Verdict::Green) {
        super::buck2_after::cargo_green(&crate::landing::check_timings::Timings::default_root(), &landed.head, send);
    }
    if !matches!(verdict, Verdict::RedMain(_)) {
        return Outcome { verdict, search: None, failing: BTreeSet::new(), superseded: run.superseded(), log: run.log };
    }
    let failing = failing(&run.log, &run.outcome);
    if settle::stale_red(&landed, &run) {
        return Outcome { verdict, search: None, failing, superseded: run.superseded(), log: run.log };
    }
    super::buck2_after::cargo_red(&crate::landing::check_timings::Timings::default_root(), &landed.head, &run.log, send);
    if already_known(&failing, known) {
        println!("north land: the after-landing run of {} is red on {} only, which main already failed at the previous verdict, so no culprit search starts (north#1631)", landed.head, failing.iter().map(Failure::label).collect::<Vec<_>>().join(", "));
        return Outcome { verdict, search: None, failing, superseded: run.superseded(), log: run.log };
    }
    let (superseded, log) = (run.superseded(), run.log.clone());
    Outcome { verdict, search: Some(Search { landed, run, failing: failing.clone() }), failing, log, superseded }
}

pub fn search(wake: &Arc<Wake>, search: Search) -> Task<Found> {
    Task::spawn(wake, move || {
        let Search { landed, run, failing } = search;
        let run = run.yielding();
        let started = telemetry::epoch();
        let (failing, named) = apart(failing, &run.outcome, &landed.lanes);
        for &at in &named {
            println!("north land: {} is a landed lane whose own range is red on a lane-scoped check of {}, which no other lane's tests or mutants enter", landed.lanes[at], landed.head);
        }
        if failing.is_empty() {
            return Found { culprits: named, span: (started, telemetry::epoch()), superseded: run.superseded(), cancelled_by: run.cancel.sender().map(str::to_owned), unreproduced: None };
        }
        let reproduced = at_the_tip(&landed, &run, &failing);
        let stopped = |unreproduced| Found { culprits: Vec::new(), span: (started, telemetry::epoch()), superseded: run.superseded(), cancelled_by: run.cancel.sender().map(str::to_owned), unreproduced };
        if run.superseded() {
            println!("north land: the culprit search of {} stopped because {} superseded it", landed.head, cancelled_by(&run));
            return stopped(None);
        }
        let failing = match reproduced {
            Ok(failing) => failing,
            Err(again) => return stopped(Some(again)),
        };
        let mut culprits = locate(&landed, &run, &failing);
        culprits.extend(named);
        culprits.sort_unstable();
        culprits.dedup();
        if run.superseded() {
            println!("north land: the culprit search of {} stopped because {} superseded it", landed.head, cancelled_by(&run));
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
    if let Verdict::Transient(detail) = &rerun.verdict {
        println!("north land: the failed checks of {} reached no verdict when rerun at the tip ({detail}), so they stand as failing (north#1631)", landed.head);
        return Ok(failing.clone());
    }
    if !matches!(rerun.verdict, Verdict::RedMain(_)) {
        println!("north land: the failed checks of {} ended {:?} when rerun at the tip, so no lane is blamed", landed.head, rerun.verdict);
        return Err(again.log);
    }
    match reproduced(failing, |failure| probe.runs(failure), &rerun.failing) {
        Some(reproduced) => Ok(reproduced),
        None => {
            println!("north land: none of the failed checks of {} failed again when rerun at the tip, so no lane is blamed", landed.head);
            Err(again.log)
        }
    }
}

/// The failing checks a rerun at the tip reproduces: a check the rerun did not run stays failing, and one it ran is
/// reproduced only when `reached` names it. `None` when the rerun ran some failing check and reproduced none of them.
pub fn reproduced(failing: &BTreeSet<Failure>, runs: impl Fn(&Failure) -> bool, reached: &BTreeSet<String>) -> Option<BTreeSet<Failure>> {
    let reproduced: BTreeSet<Failure> = failing.iter().filter(|failure| !runs(failure) || reached.contains(failure.label())).cloned().collect();
    (!failing.iter().any(&runs) || reproduced.iter().any(&runs)).then_some(reproduced)
}


fn judge_coverage(landed: &Landed, base: &str, tip: &str, log: &Path) {
    let judged = Command::new(follow::program()).args(["test", "coverage", "miss", base, tip, "--log"]).arg(log).current_dir(&landed.repo).stdin(Stdio::null()).output();
    match judged {
        Ok(output) => print!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        Err(error) => eprintln!("north land: north test coverage miss: {error}"),
    }
}

/// Whether the post-land ledger records `base`'s own after-landing run of `repository` red on one of the failed checks, which a
/// prefix probe passing that base must not overrule.
pub(super) fn red_before(ledger: &str, repository: &str, base: &str, failing: &BTreeSet<Failure>) -> bool {
    let row = ledger.lines().rev().map(|row| row.split('\t').collect::<Vec<_>>()).find(|fields| fields.get(1) == Some(&repository) && fields.get(2) == Some(&base));
    row.is_some_and(|fields| fields.get(3) == Some(&"red-main") && fields.get(4).is_some_and(|text| failing.iter().any(|failure| text.contains(failure.label()))))
}

/// One prefix check: its verdict and the failures it reached.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Probed {
    pub verdict: Verdict,
    pub failing: BTreeSet<String>,
}

fn locate(landed: &Landed, run: &Run, failing: &BTreeSet<Failure>) -> Vec<usize> {
    if red_before(&fs::read_to_string(ledger_path()).unwrap_or_default(), &landed.repository, &landed.base, failing) {
        println!("north land: the base {} of {} was already red on the failed checks in its own after-landing run, so no landed lane is blamed", landed.base, landed.head);
        return Vec::new();
    }
    let probe = Probe::of(failing);
    let check = |tree: &str| probe::probe(landed, &run.at(&landed.head, tree), tree, &probe);
    let signatures: BTreeSet<String> = failing.iter().map(|failure| failure.label().to_owned()).collect();
    if signatures.len() > 1 {
        if probed(landed, 0, &check) != Some(false) {
            println!("north land: post-land checks of {} are not green on their base {} on the failed tests, so no landed lane is blamed", landed.head, landed.base);
            return Vec::new();
        }
        let found = search::culprits(landed.tips.len(), &signatures, |counts| counts.iter().map(|&count| check(tree_at(landed, count))).collect());
        if let Some(found) = found.filter(|found| !found.is_empty()) {
            for &at in &found {
                println!("north land: {} is a landed lane whose prefix of {} first reaches a failed check", landed.lanes.get(at).map_or("?", String::as_str), landed.head);
            }
            return found;
        }
    }
    match first_red(landed.tips.len(), |count| probed(landed, count, &check)) {
        Some(at) => {
            println!("north land: {} is the first landed lane of {} red on the failed checks where its parent passes them", landed.lanes.get(at).map_or("?", String::as_str), landed.head);
            vec![at]
        }
        None => {
            println!("north land: post-land checks of {} are red, but no landed lane is blamed: the base {} is red on them or a prefix reached no verdict", landed.head, landed.base);
            Vec::new()
        }
    }
}

/// The lane whose prefix is the first red one of a red batch of `lanes` landed lanes, the full prefix being the red run:
/// the base (prefix 0) must pass, then the prefixes between are bisected. None when the base is red or a prefix reaches no verdict.
pub(super) fn first_red(lanes: usize, mut red_at: impl FnMut(usize) -> Option<bool>) -> Option<usize> {
    if lanes == 0 || red_at(0)? {
        return None;
    }
    let (mut green, mut red) = (0, lanes);
    while red - green > 1 {
        let middle = green + (red - green) / 2;
        if red_at(middle)? {
            red = middle;
        } else {
            green = middle;
        }
    }
    Some(red - 1)
}

fn tree_at(landed: &Landed, count: usize) -> &str {
    if count == 0 { landed.base.as_str() } else { landed.tips[count - 1].as_str() }
}

fn probed(landed: &Landed, count: usize, check: &dyn Fn(&str) -> Probed) -> Option<bool> {
    let probed = check(tree_at(landed, count));
    println!("north land: the failed checks with the first {count} of {} landed lanes ended {:?}", landed.tips.len(), probed.verdict);
    match probed.verdict {
        Verdict::RedMain(_) => Some(true),
        Verdict::Green => Some(false),
        _ => None,
    }
}


#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
