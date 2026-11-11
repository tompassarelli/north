use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use rustix::process::Pid;
use serde::{Deserialize, Serialize};

use super::{Checked, Landed, Outcome, POLL, Run, Verdict, judged, superseded};
use super::super::after::send;
use super::super::candidate::git;
use super::super::lander::Lander;
use super::super::candidate::change_worktrees_alone;
use super::super::outlive;
use super::super::queue;
use super::super::telemetry::epoch;

const RECORD: &str = "runner";
const STATUS: &str = "status";

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub group: i32,
    pub started: f64,
    pub deadline: u64,
    pub base: String,
    pub head: String,
    pub heads: Vec<String>,
    pub host: Option<String>,
}

fn record_path(run: &Run) -> PathBuf {
    run.outcome.with_extension(RECORD)
}

fn status_path(run: &Run) -> PathBuf {
    run.outcome.with_extension(STATUS)
}

pub fn after_owner(owner: &str) -> String {
    format!("{owner}-after")
}

pub(super) fn wrapped(run: &Run, argv: Vec<OsString>) -> Vec<OsString> {
    outlive::wrap(&status_path(run), Some(run.deadline.as_secs()), argv)
}

pub(super) fn remember(run: &Run, landed: &Landed, group: u32) {
    let record = Record { group: group.cast_signed(), started: epoch(), deadline: run.deadline.as_secs(), base: landed.base.clone(), head: landed.head.clone(), heads: landed.heads.clone(), host: run.host.clone() };
    let written = serde_json::to_string(&record).map_err(|error| error.to_string()).and_then(|text| queue::write_atomic(&record_path(run), &text).map_err(|error| error.to_string()));
    if let Err(error) = written {
        println!("north land: could not record the runner of the after-landing run of {}, so a lander restart reruns it: {error}", landed.head);
    }
}

pub(super) fn forget(run: &Run) {
    let _ = fs::remove_file(record_path(run));
    let _ = fs::remove_file(status_path(run));
}

pub fn adoptable(queue: &Path) -> Vec<(String, Record)> {
    let Ok(entries) = fs::read_dir(queue.join("logs")) else { return Vec::new() };
    let mut found = Vec::new();
    for path in entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == RECORD)) {
        let record = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Record>(&text).ok());
        let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        match record {
            Some(record) if path.with_extension(STATUS).exists() || outlive::alive(record.group) => found.push((name, record)),
            _ => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    found
}

pub fn abandon(record: &Record) {
    outlive::kill(record.group);
}

pub(super) fn started(record: &Record) -> Instant {
    Instant::now().checked_sub(Duration::from_secs_f64((epoch() - record.started).max(0.0))).unwrap_or_else(Instant::now)
}

pub(super) fn adopted(landed: &Landed, run: &Run, record: &Record) -> Checked {
    let status = status_path(run);
    let exited = || -> io::Result<Option<ExitStatus>> {
        if let Some(exited) = outlive::exit_status(&status) {
            return Ok(Some(exited));
        }
        if outlive::alive(record.group) {
            return Ok(None);
        }
        outlive::exit_status(&status).map(Some).ok_or_else(|| io::Error::other("the adopted runner ended without writing its exit status"))
    };
    let deadline = started(record) + run.deadline;
    let checked = match Pid::from_raw(record.group) {
        Some(group) => ended(supervise(group, deadline, &run.cancel, exited), "the adopted post-land runner", landed, run),
        None => Verdict::Transient(format!("the after-landing runner of {} recorded no process group", landed.head)).into(),
    };
    forget(run);
    let _ = change_worktrees_alone(&landed.repo, &["worktree", "remove", "--force", &run.worktree.to_string_lossy()]);
    checked
}

pub fn adopt(lander: &Lander, landed: Landed, record: Record, cancel: Arc<AtomicBool>) -> std::thread::JoinHandle<Outcome> {
    println!("north land: adopting the after-landing run of {} that a stopped lander started; its runner is still alive or ended with a status", landed.head);
    let run = super::after_run(lander, &landed, record.host.clone(), cancel, Duration::from_secs(record.deadline));
    std::thread::spawn(move || {
        let checked = adopted(&landed, &run, &record);
        let verdict = super::settle_from(&landed, &run, started(&record), checked, send, |command| super::offload_rerun(&landed, &run, command));
        super::outcome(landed, run, verdict)
    })
}

pub(super) fn ended(supervised: std::io::Result<Ended>, program: &str, landed: &Landed, run: &Run) -> Checked {
    match supervised {
        Ok(Ended::Exited(status)) => judged(status, run).into(),
        Ok(Ended::Cancelled) => superseded(landed).into(),
        Ok(Ended::Expired) => Checked { verdict: Verdict::Transient(format!("post-land checks passed their {} s deadline and were killed with their process group, so they reached no verdict; log: {}", run.deadline.as_secs(), run.log.display())), expired: true },
        Err(error) => Verdict::Transient(format!("{program}: {error}")).into(),
    }
}

pub(super) enum Ended {
    Exited(ExitStatus),
    Expired,
    Cancelled,
}

pub(super) fn supervise(group: rustix::process::Pid, deadline: Instant, cancel: &AtomicBool, mut exited: impl FnMut() -> std::io::Result<Option<ExitStatus>>) -> std::io::Result<Ended> {
    loop {
        if let Some(status) = exited()? {
            return Ok(Ended::Exited(status));
        }
        let cancelled = cancel.load(Ordering::Relaxed);
        if cancelled || Instant::now() >= deadline {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
            return Ok(if cancelled { Ended::Cancelled } else { Ended::Expired });
        }
        std::thread::sleep(POLL);
    }
}

pub fn reap_scratch(repo: &Path, queue: &Path) -> Vec<PathBuf> {
    let listed = git(repo, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    let live: Vec<String> = adoptable(queue).into_iter().map(|(name, _)| name).collect();
    let scratch: Vec<PathBuf> = listed
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|path| path.parent() == Some(queue) && path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("post-land") && !live.iter().any(|live| name.to_string_lossy() == live.as_str())))
        .collect();
    for path in &scratch {
        let _ = change_worktrees_alone(repo, &["worktree", "remove", "--force", &path.to_string_lossy()]);
    }
    scratch
}
