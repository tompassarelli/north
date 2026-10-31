//! Registered waits: a worker that starts a run longer than a few minutes names it here and parks,
//! holding no slot; `settle` revives it with the run's result once the run is over.
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};

use super::runtime::now_ms;

const USAGE: &str = "a run is job:NAME[@HOST], farm:RUN_ID, branch:BRANCH, capture:PATH or pid:PID@START";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum Run {
    Job { name: String, host: Option<String> },
    Farm(String),
    Branch(String),
    Capture(PathBuf),
    Process { pid: u32, started: u64 },
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
            "pid" => {
                let (pid, started) = id.split_once('@').ok_or_else(usage)?;
                Ok(Self::Process { pid: pid.parse().map_err(|_| usage())?, started: started.parse().map_err(|_| usage())? })
            }
            _ => Err(usage()),
        }
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
            Self::Process { pid, started } => write!(out, "pid:{pid}@{started}"),
        }
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

/// Whether a run is over, and how it ended.
pub trait Probe {
    fn result(&self, wait: &Registered) -> Option<String>;
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
        if waits.iter().all(|wait| wait.run != run) {
            waits.push(Registered { run, cwd: cwd.into(), since_ms: now_ms(), label: label.map(str::to_owned) });
        }
        self.store(session, &waits)
    }

    fn release(&self, session: &str, run: &Run) -> NorthResult<()> {
        let kept: Vec<Registered> = self.of(session).into_iter().filter(|wait| wait.run != *run).collect();
        self.store(session, &kept)
    }

    /// Tells each session whose registered run is over what it was and how it ended, then forgets
    /// the wait; `tell` revives a parked session. A wait whose message fails stays for the next tick.
    pub fn settle(&self, probe: &dyn Probe, tell: &dyn Fn(&str, &str) -> NorthResult<()>) -> Vec<String> {
        let mut told = Vec::new();
        for (session, waits) in self.all() {
            for wait in waits {
                let Some(result) = probe.result(&wait) else { continue };
                let named = wait.label.as_ref().map_or_else(|| wait.run.to_string(), |label| format!("{} `{label}`", wait.run));
                let text = format!("[north: the run you waited on is over] {named}: {result}. Read its output and continue to a final report.");
                if tell(&session, &text).is_ok() && self.release(&session, &wait.run).is_ok() {
                    told.push(format!("{session}: {} {result}", wait.run));
                }
            }
        }
        told
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct Over(Vec<&'static str>);

    impl Probe for Over {
        fn result(&self, wait: &Registered) -> Option<String> {
            self.0.contains(&wait.run.to_string().as_str()).then(|| "exit 0".to_owned())
        }
    }

    fn waits() -> (tempfile::TempDir, Waits) {
        let root = tempfile::tempdir().unwrap();
        let waits = Waits::new(root.path().join(".waits"));
        (root, waits)
    }

    #[test]
    fn a_run_round_trips_through_its_text_and_refuses_an_unnamed_kind() {
        for text in ["job:nexus-build", "job:build@nexus", "farm:123456", "branch:claude/land-x", "capture:/tmp/batch/done", "pid:4242@99"] {
            assert_eq!(text.parse::<Run>().unwrap().to_string(), text);
        }
        assert!("soak:1".parse::<Run>().is_err() && "job:".parse::<Run>().is_err() && "pid:4242".parse::<Run>().is_err() && "nexus".parse::<Run>().is_err());
    }

    #[test]
    fn a_finished_run_tells_its_session_the_run_and_result_once_and_a_running_one_stays() {
        let (_root, waits) = waits();
        waits.register("north-1", "farm:42".parse().unwrap(), Path::new("/repo"), None).unwrap();
        waits.register("north-1", "farm:42".parse().unwrap(), Path::new("/repo"), None).unwrap();
        waits.register("north-1", "job:soak".parse().unwrap(), Path::new("/repo"), None).unwrap();
        waits.register("north-2", "branch:claude/land-y".parse().unwrap(), Path::new("/repo"), None).unwrap();
        assert_eq!(waits.of("north-1").len(), 2, "one wait per run");

        let sent = RefCell::new(Vec::new());
        let tell = |session: &str, text: &str| {
            sent.borrow_mut().push((session.to_owned(), text.to_owned()));
            Ok(())
        };
        let told = waits.settle(&Over(vec!["farm:42"]), &tell);
        assert_eq!(told, ["north-1: farm:42 exit 0"]);
        let sent = sent.into_inner();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].0 == "north-1" && sent[0].1.contains("farm:42") && sent[0].1.contains("exit 0"), "{sent:?}");
        assert_eq!(waits.of("north-1").iter().map(|wait| wait.run.to_string()).collect::<Vec<_>>(), ["job:soak"]);
        assert_eq!(waits.of("north-2").len(), 1);
        assert!(waits.settle(&Over(vec!["farm:42"]), &|_, _| Ok(())).is_empty(), "a told wait is not told twice");
    }

    #[test]
    fn a_wait_whose_revival_fails_is_kept_for_the_next_tick() {
        let (_root, waits) = waits();
        waits.register("north-1", "job:soak".parse().unwrap(), Path::new("/repo"), None).unwrap();
        let failing = |_: &str, _: &str| Err(NorthError::State("no such session".into()));
        assert!(waits.settle(&Over(vec!["job:soak"]), &failing).is_empty());
        assert_eq!(waits.of("north-1").len(), 1);
        assert_eq!(waits.settle(&Over(vec!["job:soak"]), &|_, _| Ok(())).len(), 1);
        assert!(waits.all().is_empty());
    }
}
