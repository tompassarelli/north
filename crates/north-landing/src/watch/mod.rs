use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod facts;
pub(crate) mod github;
mod landing;
pub mod sink;

pub use facts::{Event, RepoFacts, Run, StageSample, Ticket};

const USAGE: &str = "usage: north watch --once [--quiet]";
pub const HEARTBEAT: &str = "north/watch/heartbeat";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Severity {
    Info,
    Warn,
    Page,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Alert {
    pub key: String,
    pub severity: Severity,
    pub repo: Option<String>,
    pub owner: Option<String>,
    pub summary: String,
    pub since: u64,
}

#[derive(Debug, Default)]
pub struct WatchContext {
    pub now: u64,
    pub state: PathBuf,
    pub events: Vec<Event>,
    pub stages: Vec<StageSample>,
    pub tickets: Vec<Ticket>,
    pub live: BTreeSet<u32>,
    pub repos: Vec<RepoFacts>,
    pub settle: Option<Value>,
    pub unavailable: Vec<String>,
}

pub type Probe = fn(&WatchContext) -> Vec<Alert>;

pub const PROBES: &[(&str, Probe)] = &[("landing", landing::probe), ("github", github::probe)];

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|path| path.is_absolute()).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
    })
}

pub fn evaluate(context: &WatchContext) -> Vec<Alert> {
    let mut alerts: Vec<Alert> = PROBES.iter().flat_map(|(_, probe)| probe(context)).collect();
    let mut seen = BTreeSet::new();
    alerts.retain(|alert| seen.insert(alert.key.clone()));
    alerts
}

pub fn run(arguments: &[String]) -> u8 {
    let quiet = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["--once"] => false,
        ["--once", "--quiet"] | ["--quiet", "--once"] => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let state = state_home();
    let dir = state.join("north/watch");
    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!("north watch: cannot create {}: {error}", dir.display());
        return 1;
    }
    let Ok(lock) = File::create(dir.join("lock")) else {
        eprintln!("north watch: cannot open its lock");
        return 1;
    };
    if flock(&lock, FlockOperation::LockExclusive).is_err() {
        eprintln!("north watch: cannot take its lock");
        return 1;
    }
    let context = facts::gather(&state);
    let alerts = evaluate(&context);
    let lines = sink::deliver(&dir.join("incidents.json"), &alerts, &context.unavailable, context.now, &sink::Fleet::default());
    for line in lines.iter().filter(|_| !quiet) {
        println!("{line}");
    }
    if !quiet {
        println!("north watch: {} open alert(s) at {}", alerts.len(), context.now);
    }
    let _ = fs::write(state.join(HEARTBEAT), format!("{}\n", context.now));
    0
}
