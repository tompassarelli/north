use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::candidate;
use super::checks::{self, Launch, Started, Verdict};
use super::lander::Lander;
use super::outlive;
use super::queue::{self, Ticket};

const RECORD: &str = "alone-flight.json";
const WORKTREE: &str = "lander-alone";

#[derive(Clone, Serialize, Deserialize)]
pub struct Record {
    pub ticket: PathBuf,
    pub branch: String,
    pub head: String,
    pub candidate: String,
    pub base: String,
    pub log: PathBuf,
    pub program: String,
    pub group: i32,
}

pub struct Flight {
    pub record: Record,
    started: Option<Started>,
}

pub fn owner(lander: &Lander) -> String {
    format!("{}-alone", lander.offload_owner)
}

pub fn worktree(lander: &Lander) -> Result<PathBuf, String> {
    candidate::lander_worktree(&lander.repo, WORKTREE)
}

fn record_path(queue: &Path) -> PathBuf {
    queue.join("logs").join(RECORD)
}

fn status_path(log: &Path) -> PathBuf {
    log.with_extension("status")
}

pub struct Check<'a> {
    pub path: &'a Path,
    pub ticket: &'a Ticket,
    pub candidate: &'a str,
    pub base: &'a str,
    pub worktree: &'a Path,
    pub log: PathBuf,
}

pub fn start(lander: &Lander, check: Check<'_>) -> Result<Flight, Verdict> {
    let owner = owner(lander);
    let launch = Launch { offload_owner: &owner, ..Launch::of(lander) };
    let started = checks::start(&launch, check.worktree, check.base, &check.log, true, Some(&status_path(&check.log)))?;
    let record = Record {
        ticket: check.path.to_owned(),
        branch: check.ticket.branch.clone(),
        head: check.ticket.head.clone(),
        candidate: check.candidate.to_owned(),
        base: check.base.to_owned(),
        program: launch.checker.first().cloned().unwrap_or_default(),
        log: check.log,
        group: started.id().cast_signed(),
    };
    let written = serde_json::to_string(&record).map_err(|error| error.to_string()).and_then(|text| queue::write_atomic(&record_path(&lander.queue), &text).map_err(|error| error.to_string()));
    if let Err(error) = written {
        println!("north land: could not record the alone check of {}, so a lander restart reruns it: {error}", record.branch);
    }
    Ok(Flight { record, started: Some(started) })
}

pub fn adopt(queue: &Path) -> Option<Flight> {
    let path = record_path(queue);
    let record: Record = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok())?;
    if status_path(&record.log).exists() || outlive::alive(record.group) {
        println!("north land: adopting the alone check of {} on main@{} that a stopped lander started", record.branch, record.base);
        return Some(Flight { record, started: None });
    }
    let _ = fs::remove_file(&path);
    None
}

pub fn adoptable(queue: &Path) -> bool {
    fs::read_to_string(record_path(queue)).ok().and_then(|text| serde_json::from_str::<Record>(&text).ok()).is_some_and(|record| status_path(&record.log).exists() || outlive::alive(record.group))
}

impl Flight {
    pub fn checks(&self, path: &Path) -> bool {
        self.record.ticket == path
    }

    pub fn poll(&mut self, queue: &Path) -> Option<Verdict> {
        let record = &self.record;
        let verdict = match &mut self.started {
            Some(started) => started.poll()?,
            None => match outlive::exit_status(&status_path(&record.log)) {
                Some(status) => checks::verdict(&record.program, &record.log, status),
                None if outlive::alive(record.group) => return None,
                None => outlive::exit_status(&status_path(&record.log)).map_or_else(|| Verdict::Transient(format!("the adopted alone check ended without writing its exit status; log: {}", record.log.display())), |status| checks::verdict(&record.program, &record.log, status)),
            },
        };
        let _ = fs::remove_file(record_path(queue));
        let _ = fs::remove_file(status_path(&record.log));
        Some(verdict)
    }
}
