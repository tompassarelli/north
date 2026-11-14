use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod facts;
pub(crate) mod github;
mod grid;
mod landing;
pub mod sink;
mod verdicts;

pub use facts::{Event, RepoFacts, Run, StageSample};

const USAGE: &str = "usage: north ops watch --once [--quiet] | north ops watch show NAME";
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
    pub detail: Detail,
}

/// The items an alert names beyond its summary, such as the lanes whose land checks also failed: a message names at most five, and the alert's detail file lists them all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub lead: String,
    pub unit: String,
    pub items: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Follow {
    pub sha: String,
    pub contains_landing: bool,
    pub at: u64,
}

#[derive(Debug, Default)]
pub struct WatchContext {
    pub now: u64,
    pub state: PathBuf,
    pub events: Vec<Event>,
    pub stages: Vec<StageSample>,
    pub live: BTreeSet<u32>,
    pub grid: grid::Grid,
    pub outcomes: Vec<verdicts::Outcome>,
    pub repos: Vec<RepoFacts>,
    pub settle: Option<Value>,
    pub follow: Option<Follow>,
    pub unavailable: Vec<String>,
    pub defers_fleet_alarms: bool,
}

pub type Probe = fn(&WatchContext) -> Vec<Alert>;

pub const PROBES: &[(&str, Probe)] = &[("landing", landing::probe), ("github", github::probe), ("grid", grid::probe), ("verdicts", verdicts::probe)];

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
        ["show", name] => return show(name),
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let state = state_home();
    let dir = state.join("north/watch");
    if let Err(error) = fs::create_dir_all(&dir) {
        eprintln!("north ops watch: cannot create {}: {error}", dir.display());
        return 1;
    }
    let Ok(lock) = File::create(dir.join("lock")) else {
        eprintln!("north ops watch: cannot open its lock");
        return 1;
    };
    if flock(&lock, FlockOperation::LockExclusive).is_err() {
        eprintln!("north ops watch: cannot take its lock");
        return 1;
    }
    let context = facts::gather(&state);
    let alerts = evaluate(&context);
    let lines = sink::deliver(&state, &dir.join("incidents.json"), &alerts, &context.unavailable, context.now, &sink::Fleet::default());
    for line in lines.iter().filter(|_| !quiet) {
        println!("{line}");
    }
    if !quiet {
        println!("north ops watch: {} open alert(s) at {}", alerts.len(), context.now);
    }
    let _ = fs::write(state.join(HEARTBEAT), format!("{}\n", context.now));
    0
}

fn show(name: &str) -> u8 {
    let file = sink::detail_file(&state_home().join("north/watch"), &sink::detail_name(name));
    match fs::read_to_string(&file) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(error) => {
            eprintln!("north ops watch: no detail for {name} at {}: {error}", file.display());
            1
        }
    }
}
