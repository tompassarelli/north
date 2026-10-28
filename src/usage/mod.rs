//! `north usage`: the latest reading of each account window in the threads ledger's usage table, as the
//! `agents usage` table (with the plan, reset and burn columns and the cost lines) or, with `--json`,
//! the same rows with whether each is older than `usage_stale_min`.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

mod burn;
mod costs;
mod py;
mod render;
mod settings;
mod store;
mod table;
#[cfg(test)]
mod tests;
mod web;

const USAGE: &str = "usage: north usage [--json]";

pub(crate) struct Places {
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

pub(crate) fn handles(arguments: &[String]) -> bool {
    arguments.iter().all(|argument| argument == "--json" || argument == "--local")
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    if !handles(arguments) {
        eprintln!("{USAGE}");
        return 2;
    }
    let json = arguments.iter().any(|argument| argument == "--json");
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let source = store::Source::locate(&home, std::env::var_os("THREADS_DB"));
    let places = Places::from_env();
    let outcome = store::open(&source, &crate::threads_client::refresh_mirrored_snapshot).and_then(|db| {
        let now = Timestamp::now();
        if json { report(&db, &places.policy, now).map(|text| (text, 0)) } else { table::render(&db, &places, now) }
    });
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
