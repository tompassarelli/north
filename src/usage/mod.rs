//! `north usage --json`: the latest reading of each account window in the threads ledger's usage table,
//! each with whether it is older than `usage_stale_min`, printed as `agents usage --json` prints it.
use std::collections::BTreeSet;
use std::path::Path;

use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

mod render;
mod settings;
mod store;
#[cfg(test)]
mod tests;

const USAGE: &str = "usage: north usage --json";

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments != ["--json"] {
        eprintln!("{USAGE}");
        return 2;
    }
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let source = store::Source::locate(&home, std::env::var_os("THREADS_DB"));
    let outcome = store::open(&source).and_then(|db| report(&db, &crate::pace::target::policy_path(), Timestamp::now()));
    match outcome {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("agents usage: {error}");
            1
        }
    }
}

fn report(db: &Connection, policy: &Path, now: Timestamp) -> Result<String, String> {
    let shown = Shown { known: settings::known_providers(), hosted: settings::hosted_claude(policy) };
    let stale_after = settings::stale_minutes();
    let rows = store::latest(db)?
        .into_iter()
        .filter(|row| shown.admits(row.text("provider"), row.text("account")))
        .map(|row| {
            let stale = row.age_minutes(now)?.is_none_or(|age| age == 0.0 || age > stale_after);
            Ok(render::object(row, stale))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(format!("[{}]", rows.join(", ")))
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
