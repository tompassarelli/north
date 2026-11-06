use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::after::{deadline, follow, send};
use super::{blame, culprit};
use super::candidate::{change_worktrees_alone, git};
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::setting;
use super::slots::select::OFFLOAD_SLOTS_BUSY;
use super::telemetry;

const TAIL_LINES: usize = 20;
const ALARMED: &str = "proxy";
const POLL: Duration = Duration::from_millis(50);

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
    pub culprit: Option<usize>,
    pub search: Option<(f64, f64)>,
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
}

#[derive(Clone, Default)]
pub struct Hosts(Arc<Mutex<Vec<String>>>);

struct Claim {
    hosts: Hosts,
    host: Option<String>,
}

impl Hosts {
    fn claim(&self, repo: &Path, head: &str) -> Claim {
        let mut taken = self.0.lock().unwrap_or_else(PoisonError::into_inner);
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
        Claim { hosts: self.clone(), host }
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let Some(host) = &self.host else { return };
        let mut taken = self.hosts.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = taken.iter().position(|held| held == host) {
            taken.remove(at);
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
            host: None,
        }
    }
}

pub fn start(lander: &Lander, hosts: &Hosts, base: &str, head: &str, heads: Vec<String>, lanes: Vec<String>, tips: Vec<String>) -> std::thread::JoinHandle<Outcome> {
    let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let landed = Landed { repo: lander.repo.clone(), repository: telemetry::repository(&origin), base: base.to_owned(), head: head.to_owned(), heads, lanes, tips };
    let run = Run {
        runner: vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--dry".to_owned(), "1".to_owned()],
        worktree: lander.queue.join("post-land"),
        log: lander.queue.join("logs").join("post-land"),
        outcome: lander.queue.join("logs").join("post-land"),
        ledger: state_home().join("safe-push").join("post-land.tsv"),
        offload_owner: lander.offload_owner.clone(),
        capacity: setting::landing(&lander.repo, "refs/remotes/origin/main", "landing.capacity"),
        deadline: deadline(&lander.repo),
        host: None,
    }
    .at(head, head);
    let hosts = hosts.clone();
    std::thread::spawn(move || {
        let claim = hosts.claim(&landed.repo, &landed.head);
        let run = Run { host: claim.host.clone(), ..run };
        let verdict = settle(&landed, &run, send);
        println!("north land: post-land checks of {} ended {verdict:?}", landed.head);
        if !matches!(verdict, Verdict::RedMain(_)) {
            return Outcome { verdict, culprit: None, search: None };
        }
        let started = telemetry::epoch();
        let culprit = locate(&landed, &run);
        if let Some(at) = culprit {
            judge_coverage(&landed, if at == 0 { &landed.base } else { &landed.tips[at - 1] }, &landed.tips[at], &run.at(&landed.head, &landed.tips[at]).log);
        }
        Outcome { verdict, culprit, search: Some((started, telemetry::epoch())) }
    })
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
    let started = std::time::Instant::now();
    let Checked { verdict, expired } = check(landed, &landed.head, run);
    record(run, landed, &verdict, expired, started.elapsed().as_secs());
    if let Verdict::RedMain(detail) = &verdict {
        let message = format!("north land: red main {} in {}: post-land check failed: {detail}; landed lanes: {}\n{}", landed.head, landed.repository, landed.lanes.join(", "), tail(&run.log));
        if !alarm(ALARMED, &message) {
            eprintln!("north land: could not alarm {ALARMED} about red main {}", landed.head);
        }
    }
    verdict
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
    let mut command = Command::new(program);
    command.args(prefix).args(["--base", &landed.base, "--head", &landed.head, "--tree", tree, "--outcome"]).arg(&run.outcome).current_dir(&run.worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env("NORTH_OFFLOAD_OWNER", &run.offload_owner).env("SAFE_PUSH_LAND_CAPACITY", &run.capacity);
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
    command.process_group(0);
    match supervise(&mut command, run.deadline) {
        Ok(Some(status)) => judged(status, run).into(),
        Ok(None) => Checked { verdict: Verdict::Transient(format!("post-land checks passed their {} s deadline and were killed with their process group, so they reached no verdict; log: {}", run.deadline.as_secs(), run.log.display())), expired: true },
        Err(error) => Verdict::Transient(format!("{program}: {error}")).into(),
    }
}

fn supervise(command: &mut Command, limit: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
    let mut child = command.spawn()?;
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            if let Some(pid) = rustix::process::Pid::from_raw(child.id().cast_signed()) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::Kill);
            }
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(POLL);
    }
}

fn judged(status: std::process::ExitStatus, run: &Run) -> Verdict {
    match status.code() {
        _ if status.success() => Verdict::Green,
        Some(126 | 127) => Verdict::Transient(format!("post-land checks could not start their command (exit {status}), so they reached no verdict; log: {}", run.log.display())),
        Some(75) => Verdict::Transient(format!("post-land checks exited 75; log: {}", run.log.display())),
        Some(OFFLOAD_SLOTS_BUSY) => Verdict::Transient(format!("every build-host slot stayed busy, so the post-land checks reached no verdict; log: {}", run.log.display())),
        _ => Verdict::RedMain(failed_check(&run.outcome).unwrap_or_else(|| format!("exit {status}; log: {}", run.log.display()))),
    }
}

fn failed_check(outcome: &Path) -> Option<String> {
    let text = fs::read_to_string(outcome).ok()?;
    let failed: Vec<String> = text.lines().filter_map(|line| {
        let fields: Vec<&str> = line.split('\t').collect();
        (fields.first() == Some(&"event") && fields.get(2) == Some(&"failed")).then(|| fields.get(3).copied().unwrap_or_default().to_owned())
    }).collect();
    (!failed.is_empty()).then(|| failed.join("; "))
}

fn tail(log: &Path) -> String {
    let text = fs::read_to_string(log).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(TAIL_LINES)..].join("\n")
}

fn record(run: &Run, landed: &Landed, verdict: &Verdict, expired: bool, seconds: u64) {
    if matches!(verdict, Verdict::Transient(_)) && !expired {
        return;
    }
    let (class, detail) = verdict.class();
    if let Some(parent) = run.ledger.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let covered_by = |head: &str| if head == landed.head { "" } else { landed.head.as_str() };
    let lines: String = landed.heads.iter().map(|head| format!("{}\t{}\t{head}\t{class}\t{}\t{}\t{seconds}\t{}\n", telemetry::seconds(), landed.repository, detail.replace(['\t', '\n'], " "), landed.lanes.join(","), covered_by(head))).collect();
    if let Err(error) = OpenOptions::new().create(true).append(true).open(&run.ledger).and_then(|mut file| file.write_all(lines.as_bytes())) {
        eprintln!("north land: could not record the post-land verdict of {}: {error}", landed.head);
    }
}

#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
