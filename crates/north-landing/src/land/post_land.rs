use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::after::{follow, send};
use super::candidate::git;
use super::checks::Verdict;
use super::lander::{Lander, state_home};
use super::telemetry;

const TAIL_LINES: usize = 20;
const ALARMED: &str = "proxy";

pub struct Landed {
    pub repo: PathBuf,
    pub repository: String,
    pub base: String,
    pub head: String,
    pub lanes: Vec<String>,
}

pub struct Run {
    pub runner: Vec<String>,
    pub worktree: PathBuf,
    pub log: PathBuf,
    pub outcome: PathBuf,
    pub ledger: PathBuf,
}

pub fn start(lander: &Lander, base: &str, head: &str, lanes: Vec<String>) {
    let origin = git(&lander.repo, &["remote", "get-url", "origin"]).unwrap_or_default();
    let landed = Landed { repo: lander.repo.clone(), repository: telemetry::repository(&origin), base: base.to_owned(), head: head.to_owned(), lanes };
    let short = &head[..head.len().min(12)];
    let run = Run {
        runner: vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--dry".to_owned(), "1".to_owned()],
        worktree: lander.queue.join(format!("post-land-{short}")),
        log: lander.queue.join("logs").join(format!("post-land-{short}.log")),
        outcome: lander.queue.join("logs").join(format!("post-land-{short}.outcome")),
        ledger: state_home().join("safe-push").join("post-land.tsv"),
    };
    std::thread::spawn(move || {
        let verdict = settle(&landed, &run, send);
        println!("north land: post-land checks of {} ended {verdict:?}", landed.head);
    });
}

pub fn settle(landed: &Landed, run: &Run, alarm: impl Fn(&str, &str) -> bool) -> Verdict {
    let verdict = check(landed, run);
    record(run, landed, &verdict);
    if let Verdict::RedMain(detail) = &verdict {
        let message = format!("north land: red main {} in {}: post-land check failed: {detail}; landed lanes: {}\n{}", landed.head, landed.repository, landed.lanes.join(", "), tail(&run.log));
        if !alarm(ALARMED, &message) {
            eprintln!("north land: could not alarm {ALARMED} about red main {}", landed.head);
        }
    }
    verdict
}

fn check(landed: &Landed, run: &Run) -> Verdict {
    let Some((program, prefix)) = run.runner.split_first() else { return Verdict::Transient("no post-land runner".to_owned()) };
    let _ = git(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    if let Err(error) = git(&landed.repo, &["worktree", "add", "--detach", &run.worktree.to_string_lossy(), &landed.head]) {
        return Verdict::Transient(format!("post-land worktree: {error}"));
    }
    let verdict = execute(program, prefix, landed, run);
    let _ = git(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    verdict
}

fn execute(program: &str, prefix: &[String], landed: &Landed, run: &Run) -> Verdict {
    if let Some(parent) = run.log.parent()
        && let Err(error) = fs::create_dir_all(parent)
    {
        return Verdict::Transient(format!("{}: {error}", parent.display()));
    }
    let _ = fs::remove_file(&run.outcome);
    let (stdout, stderr) = match File::create(&run.log).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok(files) => files,
        Err(error) => return Verdict::Transient(format!("{}: {error}", run.log.display())),
    };
    let mut command = Command::new(program);
    command.args(prefix).args(["--base", &landed.base, "--head", &landed.head, "--outcome"]).arg(&run.outcome).current_dir(&run.worktree).stdin(Stdio::null()).stdout(stdout).stderr(stderr).env("NORTH_OFFLOAD_OWNER", super::lander::OFFLOAD_OWNER);
    for name in super::checks::SCRUBBED {
        command.env_remove(name);
    }
    match command.status() {
        Ok(status) if status.success() => Verdict::Green,
        Ok(status) if status.code() == Some(75) => Verdict::Transient(format!("post-land checks exited 75; log: {}", run.log.display())),
        Ok(status) => Verdict::RedMain(failed_check(&run.outcome).unwrap_or_else(|| format!("exit {status}; log: {}", run.log.display()))),
        Err(error) => Verdict::Transient(format!("{program}: {error}")),
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

fn record(run: &Run, landed: &Landed, verdict: &Verdict) {
    let (class, detail) = match verdict {
        Verdict::Green => ("green", String::new()),
        Verdict::RedMain(detail) => ("red-main", detail.clone()),
        Verdict::Transient(detail) => ("transient", detail.clone()),
        Verdict::Refused(detail) => ("refused", detail.clone()),
        Verdict::Deferred(detail) => ("deferred", detail.clone()),
        Verdict::BrokenBase(detail) => ("broken-base", detail.clone()),
    };
    if let Some(parent) = run.ledger.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let line = format!("{}\t{}\t{}\t{class}\t{}\t{}\n", telemetry::seconds(), landed.repository, landed.head, detail.replace(['\t', '\n'], " "), landed.lanes.join(","));
    if let Err(error) = OpenOptions::new().create(true).append(true).open(&run.ledger).and_then(|mut file| file.write_all(line.as_bytes())) {
        eprintln!("north land: could not record the post-land verdict of {}: {error}", landed.head);
    }
}

#[cfg(test)]
#[path = "post_land_tests.rs"]
mod tests;
