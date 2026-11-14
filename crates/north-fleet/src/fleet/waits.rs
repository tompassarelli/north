//! Registered waits: a worker that starts a run longer than a few minutes names it here and parks,
//! holding no slot; `settle` revives it with the run's result once the run is over.
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;

use north_base::platform::process::run_command_within;
use north_resume::OVER;

use super::runtime::now_ms;
use super::store::{WaitKind, WaitRequest};

const USAGE: &str = "a run is job:NAME[@HOST], farm:RUN_ID, branch:BRANCH, capture:PATH, runtime:SHA, pid:PID@START or usage:ACCOUNT@TIME";
const FARM_DEADLINE_SECS: u64 = 4 * 3600;
const BRANCH_DEADLINE_SECS: u64 = 24 * 3600;
const CAPTURE_DEADLINE_SECS: u64 = 4 * 3600;
const RUNTIME_DEADLINE_SECS: u64 = 24 * 3600;
const ANCESTRY: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Run {
    Job { name: String, host: Option<String> },
    Farm(String),
    Branch(String),
    Capture(PathBuf),
    Runtime(String),
    Process { pid: u32, started: u64 },
    Usage { account: String, at_ms: u64 },
}

impl FromStr for Run {
    type Err = NorthError;

    fn from_str(text: &str) -> NorthResult<Self> {
        let usage = || NorthError::Usage(format!("{text:?} names no run: {USAGE}"));
        let (kind, id) = text.split_once(':').filter(|(_, id)| !id.is_empty()).ok_or_else(usage)?;
        match kind {
            "job" => Ok(match id.split_once('@') {
                Some((name, host)) => Self::Job { name: name.into(), host: Some(host.into()) },
                None => Self::Job { name: id.into(), host: None },
            }),
            "farm" => Ok(Self::Farm(id.into())),
            "branch" => Ok(Self::Branch(id.into())),
            "capture" => Ok(Self::Capture(id.into())),
            "runtime" if (4..=40).contains(&id.len()) && id.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) => Ok(Self::Runtime(id.into())),
            "pid" => {
                let (pid, started) = id.split_once('@').ok_or_else(usage)?;
                Ok(Self::Process { pid: pid.parse().map_err(|_| usage())?, started: started.parse().map_err(|_| usage())? })
            }
            "usage" => {
                let (account, time) = id.split_once('@').filter(|(account, _)| !account.is_empty()).ok_or_else(usage)?;
                let at_ms = u64::try_from(Timestamp::parse(time).map_err(|_| usage())?.unix_millis()).map_err(|_| usage())?;
                Ok(Self::Usage { account: account.into(), at_ms })
            }
            _ => Err(usage()),
        }
    }
}

impl Run {
    pub fn request(&self, session: &str) -> Option<WaitRequest> {
        let (kind, key, within_secs) = match self {
            Self::Farm(id) => (WaitKind::Farm, id.clone(), FARM_DEADLINE_SECS),
            Self::Branch(branch) => (WaitKind::Branch, branch.clone(), BRANCH_DEADLINE_SECS),
            Self::Capture(path) => (WaitKind::Capture, path.display().to_string(), CAPTURE_DEADLINE_SECS),
            Self::Runtime(sha) => (WaitKind::Runtime, sha.clone(), RUNTIME_DEADLINE_SECS),
            Self::Job { .. } | Self::Process { .. } | Self::Usage { .. } => return None,
        };
        Some(WaitRequest { session: session.to_owned(), kind, key, within_secs })
    }
}

impl TryFrom<String> for Run {
    type Error = NorthError;

    fn try_from(text: String) -> NorthResult<Self> {
        text.parse()
    }
}

impl From<Run> for String {
    fn from(run: Run) -> Self {
        run.to_string()
    }
}

impl fmt::Display for Run {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Job { name, host: Some(host) } => write!(out, "job:{name}@{host}"),
            Self::Job { name, host: None } => write!(out, "job:{name}"),
            Self::Farm(id) => write!(out, "farm:{id}"),
            Self::Branch(branch) => write!(out, "branch:{branch}"),
            Self::Capture(path) => write!(out, "capture:{}", path.display()),
            Self::Runtime(sha) => write!(out, "runtime:{sha}"),
            Self::Process { pid, started } => write!(out, "pid:{pid}@{started}"),
            Self::Usage { account, at_ms } => write!(out, "usage:{account}@{}", Timestamp::from_unix_millis(i64::try_from(*at_ms).unwrap_or(i64::MAX)).iso()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settled {
    pub session: String,
    pub run: Run,
    pub result: String,
}

impl Settled {
    pub fn line(&self) -> String {
        format!("{}: {} {}", self.session, self.run, self.result)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Registered {
    pub run: Run,
    /// Where the worker was, for the probes that read a repository.
    pub cwd: PathBuf,
    pub since_ms: u64,
    /// What the run is, for the message that revives its session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl Registered {
    /// A pid wait an older session host found in its scope and labeled with the command, rather than
    /// one the session registered with `north fleet wait`; only the session's own wait wakes it, so
    /// the old run is dropped without a message (north#1055).
    fn is_unregistered_detached(&self) -> bool {
        matches!(&self.run, Run::Process { .. }) && self.label.is_some()
    }
}

/// Whether a run is over, and how it ended.
pub trait Probe {
    fn result(&self, wait: &Registered) -> Option<String>;

    /// Which of the waits that are over revives first: the lower number.
    fn precedence(&self, _session: &str, _wait: &Registered) -> u8 {
        0
    }

    fn note(&self, _session: &str, _wait: &Registered) -> Option<String> {
        None
    }

    fn heard(&self, _session: &str, _wait: &Registered) -> bool {
        true
    }
}

const DIR: &str = ".waits";

pub struct Waits {
    dir: PathBuf,
}

impl Waits {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn in_root(root: &Path) -> Self {
        Self::new(root.join(DIR))
    }

    fn file(&self, session: &str) -> PathBuf {
        self.dir.join(format!("{session}.json"))
    }

    pub fn of(&self, session: &str) -> Vec<Registered> {
        fs::read(self.file(session)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub fn all(&self) -> BTreeMap<String, Vec<Registered>> {
        let names = fs::read_dir(&self.dir).into_iter().flatten().flatten().filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json")).filter_map(|entry| Some(entry.path().file_stem()?.to_str()?.to_owned()));
        names.map(|name| (name.clone(), self.of(&name))).filter(|(_, waits)| !waits.is_empty()).collect()
    }

    fn store(&self, session: &str, waits: &[Registered]) -> NorthResult<()> {
        if waits.is_empty() {
            return match fs::remove_file(self.file(session)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            };
        }
        fs::create_dir_all(&self.dir)?;
        let scratch = self.dir.join(format!(".{session}.tmp"));
        fs::write(&scratch, serde_json::to_vec(waits)?)?;
        fs::rename(scratch, self.file(session))?;
        Ok(())
    }

    pub fn register(&self, session: &str, run: Run, cwd: &Path, label: Option<&str>) -> NorthResult<()> {
        let mut waits = self.of(session);
        if waits.iter().any(|wait| wait.run == run) {
            return Ok(());
        }
        waits.push(Registered { run, cwd: cwd.into(), since_ms: now_ms(), label: label.map(str::to_owned) });
        self.store(session, &waits)
    }

    pub fn clear_usage(&self, session: &str) -> NorthResult<()> {
        let kept: Vec<Registered> = self.of(session).into_iter().filter(|wait| !matches!(wait.run, Run::Usage { .. })).collect();
        self.store(session, &kept)
    }

    fn release(&self, session: &str, run: &Run) -> NorthResult<()> {
        let kept: Vec<Registered> = self.of(session).into_iter().filter(|wait| wait.run != *run).collect();
        self.store(session, &kept)
    }

    /// The waits of `session` that are over, dropping a pid wait an older host registered without the session asking.
    fn over(&self, session: &str, waits: Vec<Registered>, probe: &dyn Probe) -> Vec<(u8, String, Registered, String)> {
        let mut over = Vec::new();
        for wait in waits {
            if wait.is_unregistered_detached() {
                let _ = self.release(session, &wait.run);
                continue;
            }
            if !probe.heard(session, &wait) {
                continue;
            }
            if let Some(result) = probe.result(&wait) {
                over.push((probe.precedence(session, &wait), session.to_owned(), wait, result));
            }
        }
        over
    }

    /// Tells each session whose registered run is over what it was and how it ended, then forgets
    /// the wait; `tell` revives a parked session. A wait whose message fails stays for the next tick.
    pub fn settle(&self, probe: &dyn Probe, exists: &dyn Fn(&str) -> bool, tell: &dyn Fn(&str, &str) -> NorthResult<()>) -> Vec<Settled> {
        let mut over = Vec::new();
        for (session, waits) in self.all() {
            if !exists(&session) {
                let _ = self.store(&session, &[]);
                continue;
            }
            over.extend(self.over(&session, waits, probe));
        }
        over.sort_by_key(|(precedence, _, wait, _)| (*precedence, wait.since_ms));
        let mut told = Vec::new();
        for (_, session, wait, result) in over {
            let note = probe.note(&session, &wait);
            if tell(&session, &revival(&wait, &result, note.as_deref())).is_ok() && self.release(&session, &wait.run).is_ok() {
                told.push(Settled { session, run: wait.run, result });
            }
        }
        told
    }
}

pub fn runtime_holds(live: Option<&Path>, sha: &str, cwd: &Path) -> Option<String> {
    let live = live?.file_name()?.to_str()?;
    let mut ancestry = Command::new("git");
    ancestry.args(["merge-base", "--is-ancestor", sha, live]).current_dir(cwd);
    run_command_within(ancestry, None, ANCESTRY).filter(|out| out.status.success()).map(|_| format!("is in the live runtime {live}"))
}

fn revival(wait: &Registered, result: &str, note: Option<&str>) -> String {
    if let Run::Usage { account, .. } = &wait.run {
        return match note {
            Some(note) => format!("{}\n{note}", resume_message(account)),
            None => resume_message(account),
        };
    }
    let named = wait.label.as_ref().map_or_else(|| wait.run.to_string(), |label| format!("{} `{label}`", wait.run));
    format!("{OVER} {named}: {result}. Read its output and continue to a final report.")
}


pub fn resume_message(account: &str) -> String {
    format!(
        "[north: account {account}'s usage limit has reset] Your last turn stopped on the limit. Run `git fetch origin`, then check whether your lane is on origin/main. \
         If it is not, rebase it onto origin/main, re-run the targeted checks and re-enqueue it with `safe-push --to main`. \
         If it is, report `Done:` with the landed sha."
    )
}

#[cfg(test)]
#[path = "waits_tests.rs"]
mod tests;
