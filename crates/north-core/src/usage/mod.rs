//! `north usage`: the latest reading of each account window in the threads ledger's usage table, as the
//! `agents usage` table (with the plan, reset and burn columns and the cost lines) or, with `--json`,
//! the same rows with whether each is older than `usage_stale_min`.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

mod burn;
mod costs;
mod ctx;
mod fast;
mod gate;
mod py;
mod refresh;
mod render;
mod settings;
mod store;
mod table;
#[cfg(test)]
mod tests;
mod web;

const USAGE: &str = "usage: north usage [--refresh [--local]] [--json] | --fast PROVIDER [--cached] | --write-pace-gate";
const PACED: [&str; 2] = ["claude", "gemini"];

pub struct Places {
    pub policy: PathBuf,
    pub operator: PathBuf,
    pub state: PathBuf,
}

impl Places {
    fn from_env() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let set = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self {
            policy: crate::pace::target::policy_path(),
            operator: set("AGENTS_OPERATOR_DIR").unwrap_or_else(|| home.join("code/south/main/agents")),
            state: set("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local/state")),
        }
    }
}

enum Action {
    WritePaceGate,
    Fast { provider: String, cached: bool },
    Show { refresh: bool, json: bool, remote: bool },
}

impl Action {
    fn parse(arguments: &[String]) -> Option<Self> {
        let (mut json, mut local, mut refresh, mut write_gate, mut cached, mut fast) = (false, false, false, false, false, None);
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--json" => json = true,
                "--local" => local = true,
                "--refresh" => refresh = true,
                "--write-pace-gate" => write_gate = true,
                "--cached" => cached = true,
                "--fast" => fast = Some(rest.next()?.clone()),
                "--domain" => {
                    rest.next()?;
                }
                _ => return None,
            }
        }
        if write_gate {
            return Some(Self::WritePaceGate);
        }
        Some(match fast {
            Some(provider) => Self::Fast { provider, cached },
            None => Self::Show { refresh, json, remote: !local },
        })
    }

    fn writes(&self) -> bool {
        matches!(self, Self::Fast { cached: false, .. } | Self::Show { refresh: true, .. })
    }

    fn perform(&self, db: &Connection, source: &store::Source, places: &Places, host: &Host) -> Result<(String, u8), String> {
        match self {
            Self::WritePaceGate => gate::write(db, &PACED, places, clock()).map(|verdicts| (verdicts.to_string(), 0)),
            Self::Fast { provider, cached } => fast::decide(db, source, places, provider, *cached, host),
            Self::Show { refresh, json, remote } => {
                if *refresh {
                    refresh::refresh_readings(db, source, places, *remote, host)?;
                }
                let now = clock();
                if *json { report(db, &places.policy, now).map(|text| (text, 0)) } else { table::render(db, places, now) }
            }
        }
    }
}

pub fn handles(arguments: &[String]) -> bool {
    Action::parse(arguments).is_some()
}

fn clock() -> Timestamp {
    std::env::var("USAGE_NOW").ok().and_then(|text| Timestamp::parse_rfc3339(&text)).unwrap_or_else(Timestamp::now)
}

pub fn write_pace_gate(db: &Connection) -> Result<serde_json::Value, String> {
    gate::write(db, &PACED, &Places::from_env(), Timestamp::now())
}

pub struct Host<'a> {
    pub name: String,
    pub refresh_snapshot: &'a dyn Fn(),
    pub ingest: &'a dyn Fn(&[u8]) -> Result<(), String>,
    pub run: &'a (dyn Fn(std::process::Command, std::time::Duration) -> Option<std::process::Output> + Sync),
    pub ssh: &'a dyn Fn(&[&str]) -> Vec<std::ffi::OsString>,
    pub harness_lib: &'a dyn Fn() -> Result<PathBuf, String>,
}

pub fn run(arguments: &[String], host: &Host) -> u8 {
    let Some(action) = Action::parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let source = store::Source::locate(&home, std::env::var_os("THREADS_DB"));
    let places = Places::from_env();
    let open = if action.writes() { store::open_writable } else { store::open };
    let outcome = open(&source, host.refresh_snapshot).and_then(|db| action.perform(&db, &source, &places, host));
    match outcome {
        Ok((text, code)) => {
            println!("{text}");
            code
        }
        Err(error) if error.starts_with("NorthThreads:") => {
            eprintln!("{error}");
            1
        }
        Err(error) => {
            eprintln!("agents usage: {error}");
            1
        }
    }
}

fn report(db: &Connection, policy: &Path, now: Timestamp) -> Result<String, String> {
    let rows = readings(db, policy)?
        .into_iter()
        .map(|row| {
            let stale = row.age_minutes(now)?.is_none_or(|age| age == 0.0 || age > settings::stale_minutes());
            Ok(render::object(row, stale))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!("[{}]", rows.join(", ")))
}

fn readings(db: &Connection, policy: &Path) -> Result<Vec<store::Row>, String> {
    let shown = Shown { known: settings::known_providers(), hosted: settings::hosted_claude(policy) };
    Ok(store::latest(db)?.into_iter().filter(|row| shown.admits(row.text("provider"), row.text("account"))).collect())
}

struct Shown {
    known: BTreeSet<String>,
    hosted: BTreeSet<String>,
}

impl Shown {
    fn admits(&self, provider: Option<&str>, account: Option<&str>) -> bool {
        let Some(provider) = provider.filter(|provider| self.known.contains(*provider)) else {
            return false;
        };
        provider != "claude" || self.hosted.is_empty() || account.is_some_and(|account| self.hosted.contains(account))
    }
}

pub fn ledger_path(home: &Path, explicit: Option<std::ffi::OsString>) -> PathBuf {
    store::Source::locate(home, explicit).path().to_owned()
}

pub fn open_ledger(home: &Path, explicit: Option<std::ffi::OsString>, refresh: &dyn Fn()) -> Result<Connection, String> {
    store::open(&store::Source::locate(home, explicit), refresh)
}

pub fn json_readings(db: &Connection, now: Timestamp) -> Result<Vec<serde_json::Value>, String> {
    let text = report(db, &crate::pace::target::policy_path(), now)?;
    serde_json::from_str(&text).map_err(|error| format!("unreadable usage readings: {error}"))
}
