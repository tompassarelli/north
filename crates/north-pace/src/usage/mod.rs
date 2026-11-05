//! `north usage`: the latest reading of each account window in the threads ledger's usage table, as a short
//! table of each account's windows, weekly reset and outlook with this week's cost; with `--verbose`, one
//! detail line per account with every window, billing, reset, burn and cost line; with `--json`, the same
//! rows with whether each is older than `usage_stale_min`.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

mod burn;
mod costs;
pub mod events;
mod ctx;
mod detail;
mod fast;
mod gate;
pub mod breaker;
pub mod decide;
pub mod outlook;
pub mod profiles;
mod py;
mod refresh;
mod render;
mod settings;
pub mod shell;
mod store;
mod plain;
mod table;
#[cfg(test)]
mod tests;
mod web;

const USAGE: &str = "usage: north usage [--refresh [--local]] [--json | --verbose] | --fast PROVIDER [--cached] | --rows PROVIDER [--no-refresh] | --write-pace-gate | decide --dry-run BRIEF | sample";
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
    Sample,
    Decide { brief: PathBuf },
    WritePaceGate,
    Fast { provider: String, cached: bool },
    Show { refresh: bool, json: bool, verbose: bool, remote: bool },
}

impl Action {
    fn parse(arguments: &[String]) -> Option<Self> {
        if let [verb] = arguments
            && verb == "sample"
        {
            return Some(Self::Sample);
        }
        if let [verb, flag, brief] = arguments
            && verb == "decide"
            && flag == "--dry-run"
        {
            return Some(Self::Decide { brief: brief.into() });
        }
        let (mut json, mut verbose, mut local, mut refresh, mut write_gate, mut cached, mut fast) = (false, false, false, false, false, false, None);
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--json" => json = true,
                "--verbose" => verbose = true,
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
            None => Self::Show { refresh, json, verbose, remote: !local },
        })
    }

    fn writes(&self) -> bool {
        matches!(self, Self::Sample | Self::Fast { cached: false, .. } | Self::Show { refresh: true, .. })
    }

    fn perform(&self, db: &Connection, source: &store::Source, places: &Places, host: &Host) -> Result<(String, u8), String> {
        match self {
            Self::Sample => refresh::sample_readings(db, source, places, host).map(|rows| (format!("sampled {rows} readings"), 0)),
            Self::Decide { brief } => decide(db, places, host, brief),
            Self::WritePaceGate => gate::write(db, &PACED, places, clock()).map(|verdicts| (verdicts.to_string(), 0)),
            Self::Fast { provider, cached } => fast::decide(db, source, places, provider, *cached, host),
            Self::Show { refresh, json, verbose, remote } => {
                if *refresh {
                    refresh::refresh_readings(db, source, places, *remote, host)?;
                }
                let now = clock();
                if *json {
                    report(db, &places.policy, now).map(|text| (text, 0))
                } else if *verbose {
                    detail::render(db, places, now)
                } else {
                    table::render(&table::View { db, places, now, offset_seconds: crate::foundation::clock::local_offset_seconds() })
                }
            }
        }
    }
}

pub fn handles(arguments: &[String]) -> bool {
    rows_request(arguments).is_some() || Action::parse(arguments).is_some()
}

fn decide(db: &Connection, places: &Places, host: &Host, brief: &Path) -> Result<(String, u8), String> {
    let text = std::fs::read_to_string(brief).map_err(|error| format!("{}: {error}", brief.display()))?;
    let registry = (host.registry)()?;
    let shell = shell::Shell { db, policy: &places.policy, host: &host.name, registry: registry.as_ref(), directory: places.state.join("agents/usage") };
    shell::dry_run(&shell, &text).map(|text| (text, 0))
}

fn rows_request(arguments: &[String]) -> Option<(&str, bool)> {
    match arguments {
        [flag, provider] if flag == "--rows" => Some((provider, true)),
        [flag, provider, quiet] if flag == "--rows" && quiet == "--no-refresh" => Some((provider, false)),
        _ => None,
    }
}

fn print_rows(provider: &str, refresh: bool) -> u8 {
    let Some(rows) = refresh::adapter_rows(provider, refresh) else {
        eprintln!("{USAGE}");
        return 2;
    };
    for row in rows {
        println!("{}", serde_json::Value::Object(row));
    }
    0
}

fn clock() -> Timestamp {
    std::env::var("USAGE_NOW").ok().and_then(|text| Timestamp::parse_rfc3339(&text)).unwrap_or_else(Timestamp::now)
}

pub fn billed_account(provider: &str) -> Option<String> {
    let document = std::fs::read_to_string(crate::pace::target::policy_path()).ok().and_then(|text| text.parse::<toml_edit::DocumentMut>().ok());
    let cfg = document.map_or(serde_json::Value::Null, |document| crate::router::policy::json_value(document.as_item()));
    gate::local_account(provider, &cfg)
}

pub fn write_pace_gate(db: &Connection) -> Result<serde_json::Value, String> {
    gate::write(db, &PACED, &Places::from_env(), Timestamp::now())
}

pub type TokenHeaders = Vec<(String, Result<String, String>)>;

pub struct Host<'a> {
    pub name: String,
    pub refresh_snapshot: &'a dyn Fn(),
    pub ingest: &'a dyn Fn(&[u8]) -> Result<(), String>,
    pub run: &'a (dyn Fn(std::process::Command, std::time::Duration) -> Option<std::process::Output> + Sync),
    pub ssh: &'a dyn Fn(&[&str]) -> Vec<std::ffi::OsString>,
    pub registry: &'a dyn Fn() -> Result<Box<dyn shell::Registry>, String>,
    pub token_headers: &'a dyn Fn(&Path) -> TokenHeaders,
}

pub fn run(arguments: &[String], host: &Host) -> u8 {
    if let Some((provider, refresh)) = rows_request(arguments) {
        return print_rows(provider, refresh);
    }
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
