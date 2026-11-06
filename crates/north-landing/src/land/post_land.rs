use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use north_base::foundation::exit::NO_VERDICT_STATUS;
use north_base::foundation::offload::OWNER_ENV;

use crate::landing::red_check::{self, Failure};
use crate::platform::push_ledger;
use crate::landing::check_timings::Deadline;
use super::after::{follow, send};
use super::{blame, culprit};
use super::red_episode::Episode;
use super::candidate::{change_worktrees_alone, git};
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::setting;
use super::slots::select::OFFLOAD_SLOTS_BUSY;
use super::telemetry;

const ALARMED: &str = "proxy";
const POLL: Duration = Duration::from_millis(50);
const EPISODE: &str = "post-land-red";
const DEADLINE_SETTING: &str = "landing.after-deadline-sec";

mod runner;
pub use runner::{Record, SLICE_ENV, abandon, adopt, adoptable, after_owner, reap_scratch};
use runner::{Ended, ended, supervise};

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

pub(crate) fn rerun(lander: &Lander, base: &str, tree: &str, failing: &[String]) -> Option<BTreeSet<Failure>> {
    let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let landed = Landed { repo: lander.repo.clone(), repository: push_ledger::repository(&origin), base: base.to_owned(), head: tree.to_owned(), heads: Vec::new(), lanes: Vec::new(), tips: Vec::new() };
    let mut runner = vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--dry".to_owned(), "1".to_owned()];
    for step in failing {
        runner.extend(["--command".to_owned(), step.clone()]);
    }
    let logs = lander.queue.join("logs");
    let name = format!("sheriff-{}", &tree[..tree.len().min(12)]);
    let run = Run {
        runner,
        worktree: lander.queue.join("sheriff-probe"),
        log: logs.join(format!("{name}.log")),
        outcome: logs.join(format!("{name}.outcome")),
        ledger: PathBuf::new(),
        offload_owner: lander.offload_owner.clone(),
        capacity: setting::landing(&lander.repo, "refs/remotes/origin/main", "landing.capacity"),
        deadline: deadline(&lander.repo, &ledger_path(), &landed.repository, tree),
        host: None,
        cancel: Arc::default(),
        adoptable: false,
    };
    rerun_result(&check(&landed, tree, &run).verdict, &run.outcome, failing)
}

fn rerun_result(verdict: &Verdict, outcome: &Path, requested: &[String]) -> Option<BTreeSet<Failure>> {
    let requested: BTreeSet<Failure> = requested.iter().map(|step| Failure::Step(step.clone())).collect();
    match verdict {
        Verdict::Green => Some(BTreeSet::new()),
        Verdict::RedMain(_) => Some(steps(outcome)).filter(|failed| failed.is_subset(&requested)),
        _ => None,
    }
}

fn judge_coverage(landed: &Landed, base: &str, tip: &str, log: &Path) {
    let judged = Command::new(follow::program()).args(["coverage", "miss", base, tip, "--log"]).arg(log).current_dir(&landed.repo).stdin(Stdio::null()).output();
    match judged {
        Ok(output) => print!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        Err(error) => eprintln!("north land: north coverage miss: {error}"),
    }
}

fn locate(landed: &Landed, run: &Run) -> Option<usize> {
    let lanes = landed.tips.len();
    let room = culprit::free_slots(&landed.repo).map_or(1, |(free, _)| free);
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

pub fn settle(landed: &Landed, run: &Run, alarm: impl Fn(&str, &str) -> bool) -> Verdict {
    let started = Instant::now();
    let checked = check(landed, &landed.head, run);
    settle_from(landed, run, started, checked, alarm)
}

fn settle_from(landed: &Landed, run: &Run, started: Instant, mut checked: Checked, alarm: impl Fn(&str, &str) -> bool) -> Verdict {
    if matches!(checked.verdict, Verdict::Transient(_)) && !checked.expired && !run.superseded() {
        println!("north land: the post-land checks of {} reached no verdict; rerunning them once", landed.head);
        checked = check(landed, &landed.head, run);
    }
    let Checked { verdict, expired } = checked;
    record(run, landed, &verdict, expired, started.elapsed().as_secs());
    announce(landed, run, &verdict, alarm);
    verdict
}

fn episode_marker(run: &Run, landed: &Landed) -> PathBuf {
    run.ledger.with_file_name(format!("{EPISODE}-{}", landed.repository.replace('/', "_")))
}

fn red_message(landed: &Landed, run: &Run, detail: &str) -> String {
    let log = &run.log;
    let failing: Vec<String> = failing(log, &run.outcome).iter().map(|failure| failure.label().to_owned()).collect();
    let failing = if failing.is_empty() { detail.to_owned() } else { failing.join(", ") };
    format!("north land: main is red in {} at {}: failing {failing}; suspect commit {} (landed lanes: {}); log: {}; the sheriff acts first, this message is for when it cannot", landed.repository, landed.head, landed.head, landed.lanes.join(", "), log.display())
}

fn green_message(landed: &Landed) -> String {
    format!("north land: main is green again in {} at {}", landed.repository, landed.head)
}

fn announce(landed: &Landed, run: &Run, verdict: &Verdict, alarm: impl Fn(&str, &str) -> bool) {
    let marker = episode_marker(run, landed);
    let episode = Episode::read(&marker);
    let log = fs::read_to_string(&run.log).unwrap_or_default();
    match verdict {
        Verdict::RedMain(detail) if episode.is_none() => {
            if alarm(ALARMED, &red_message(landed, run, detail)) {
                let _ = Episode::opened(&landed.head, &log, failing(&run.log, &run.outcome)).write(&marker);
            } else {
                eprintln!("north land: could not alarm {ALARMED} about red main {}", landed.head);
            }
        }
        Verdict::Green if episode.as_ref().is_some_and(|episode| !episode.ended_by(&log, &landed.repo, &landed.head)) => {
            println!("north land: the green after-landing run of {} did not rerun the checks that turned main red, so main stays red", landed.head);
        }
        Verdict::Green if episode.is_some() => {
            if alarm(ALARMED, &green_message(landed)) {
                let _ = fs::remove_file(&marker);
            } else {
                eprintln!("north land: could not tell {ALARMED} that main is green again at {}", landed.head);
            }
        }
        _ => {}
    }
}

pub(crate) struct Checked {
    verdict: Verdict,
    expired: bool,
}

impl From<Verdict> for Checked {
    fn from(verdict: Verdict) -> Self {
        Self { verdict, expired: false }
    }
}

fn check(landed: &Landed, tree: &str, run: &Run) -> Checked {
    if run.superseded() {
        return superseded(landed).into();
    }
    let Some((program, prefix)) = run.runner.split_first() else { return Verdict::Transient("no post-land runner".to_owned()).into() };
    let _ = change_worktrees_alone(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    if let Err(error) = change_worktrees_alone(&landed.repo, &["worktree", "add", "--detach", &run.worktree.to_string_lossy(), tree]) {
        return Verdict::Transient(format!("post-land worktree: {error}")).into();
    }
    let checked = execute(program, prefix, landed, tree, run);
    let _ = change_worktrees_alone(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    checked
}

pub(crate) fn execute(program: &str, prefix: &[String], landed: &Landed, tree: &str, run: &Run) -> Checked {
    if let Some(parent) = run.log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Verdict::Transient(format!("{}: {error}", parent.display())).into();
    }
    let _ = fs::remove_file(&run.outcome);
    let (stdout, stderr) = match File::create(&run.log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Verdict::Transient(format!("{}: {error}", run.log.display())).into(),
    };
    let here = run.host.as_deref() == Some(north_base::platform::host::name().as_str()) && !run.capacity.is_empty();
    let mut argv: Vec<OsString> = Vec::new();
    if here {
        argv.extend(["bash".into(), "-euo".into(), "pipefail".into(), "-c".into(), format!("exec {} \"$@\"", run.capacity).into(), "post-land-capacity".into()]);
    }
    argv.push(program.into());
    argv.extend(prefix.iter().map(OsString::from));
    argv.extend(["--base", &landed.base, "--head", &landed.head, "--tree", tree, "--outcome"].map(OsString::from));
    argv.push(run.outcome.clone().into_os_string());
    if run.adoptable {
        argv = runner::wrapped(run, argv);
    }
    let Some((first, rest)) = argv.split_first() else { return Verdict::Transient("no post-land runner".to_owned()).into() };
    let mut command = Command::new(first);
    command.args(rest).current_dir(&run.worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env(OWNER_ENV, &run.offload_owner).env("SAFE_PUSH_LAND_CAPACITY", &run.capacity);
    match &run.host {
        Some(host) => command.env("SAFE_PUSH_LAND_RUNNER", host),
        None => command.env_remove("SAFE_PUSH_LAND_RUNNER"),
    };
    if let Some(logs) = run.log.parent() {
        command.env(super::trace::SPANS_ENV, super::trace::Spans::beside(logs).path());
    }
    for name in super::checks::SCRUBBED {
        command.env_remove(name);
    }
    command.env_remove(SLICE_ENV);
    command.process_group(0);
    let supervised = command.spawn().and_then(|mut child| {
        let Some(group) = rustix::process::Pid::from_raw(child.id().cast_signed()) else { return Err(std::io::Error::other("the runner has no process id")) };
        if run.adoptable {
            runner::remember(run, landed, child.id());
        }
        let supervised = supervise(group, Instant::now() + run.deadline, &run.cancel, || child.try_wait());
        if !matches!(supervised, Ok(Ended::Exited(_))) {
            let _ = child.kill();
            let _ = child.wait();
        }
        if run.adoptable {
            runner::forget(run);
        }
        supervised
    });
    ended(supervised, program, landed, run)
}

fn judged(status: ExitStatus, run: &Run) -> Verdict {
    match status.code() {
        _ if status.success() => Verdict::Green,
        Some(126 | 127) => Verdict::Transient(format!("post-land checks could not start their command (exit {status}), so they reached no verdict; log: {}", run.log.display())),
        Some(NO_VERDICT_STATUS) => Verdict::Transient(format!("post-land checks exited {NO_VERDICT_STATUS}; log: {}", run.log.display())),
        Some(OFFLOAD_SLOTS_BUSY) => Verdict::Transient(format!("every build-host slot stayed busy, so the post-land checks reached no verdict; log: {}", run.log.display())),
        _ if steps(&run.outcome).is_empty() => Verdict::Transient(format!("post-land checks exited {status} with no check reaching a failing verdict, so they reached no verdict; log: {}", run.log.display())),
        _ => Verdict::RedMain(failed_check(&run.outcome).unwrap_or_else(|| format!("exit {status}; log: {}", run.log.display()))),
    }
}

pub(crate) fn steps(outcome: &Path) -> BTreeSet<Failure> {
    let text = fs::read_to_string(outcome).unwrap_or_default();
    text.lines().filter_map(|line| line.strip_prefix("failed-step\t")).filter_map(|rest| rest.split('\t').next()).filter(|command| !command.is_empty()).map(|command| Failure::Step(command.to_owned())).collect()
}

pub(crate) fn failing(log: &Path, outcome: &Path) -> BTreeSet<Failure> {
    let mut failing = red_check::failures(&fs::read_to_string(log).unwrap_or_default());
    failing.extend(steps(outcome));
    failing
}

fn failed_check(outcome: &Path) -> Option<String> {
    let text = fs::read_to_string(outcome).ok()?;
    let failed: Vec<String> = text.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        (fields.first() == Some(&"event") && fields.get(2) == Some(&"failed")).then(|| fields.get(3).copied().unwrap_or_default().to_owned())
    }).collect();
    (!failed.is_empty()).then(|| failed.join("; "))
}

fn record(run: &Run, landed: &Landed, verdict: &Verdict, expired: bool, seconds: u64) {
    if matches!(verdict, Verdict::Transient(_)) && !expired {
        return;
    }
    let (class, detail) = verdict.class();
    if let Some(parent) = run.ledger.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tests = fs::read_to_string(&run.log).ok().and_then(|text| test_run_seconds(&text)).map(|seconds| seconds.to_string()).unwrap_or_default();
    let covered_by = |head: &str| if head == landed.head { "" } else { landed.head.as_str() };
    let lines: String = landed.heads.iter().map(|head| format!("{}\t{}\t{head}\t{class}\t{}\t{}\t{seconds}\t{}\t{tests}\n", telemetry::seconds(), landed.repository, detail.replace(['\t', '\n'], " "), landed.lanes.join(","), covered_by(head))).collect();
    if let Err(error) = OpenOptions::new().create(true).append(true).open(&run.ledger).and_then(|mut file| file.write_all(lines.as_bytes())) {
        eprintln!("north land: could not record the post-land verdict of {}: {error}", landed.head);
    }
}

fn test_run_seconds(log: &str) -> Option<u64> {
    let runs: Vec<f64> = log.lines().filter_map(|line| line.trim_start().strip_prefix("Summary [")?.split_once("s]")?.0.trim().parse().ok()).collect();
    (!runs.is_empty()).then(|| runs.iter().sum::<f64>().round() as u64)
}

#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
