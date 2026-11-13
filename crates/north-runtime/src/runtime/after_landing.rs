//! The after-landing verdict of a head (north#1022), read on the host that settles the fleet, where the lander runs: the lander appends one row per landed head to safe-push/post-land.tsv when its after-landing run ends, and keeps a `.runner` record in its queue's logs while the run goes on. A head's own last row decides: red refuses whatever its mark, and green counts only when the run took no reused verdict (marked fresh). With no row of its own, the last fresh row a later batch tip's run wrote for it decides; a head whose run is still recorded as running is refused, as is one with no row at all, and an unreadable ledger reads as no row. Any process running as the operator can append a row, so this verdict is only as trusted as the operator's own sessions. The switch `after_landing_green` in harness/defaults.toml, read from the running runtime's tree, turns the read off.

use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::harness::{DEFAULTS, document};
use north_base::platform::land_queue;
use serde::Deserialize;

const SWITCH: &str = "after_landing_green";
const LEDGER: &str = "safe-push/post-land.tsv";
const GREEN: &str = "green";
const FRESH: &str = "fresh";
const RUNNER: &str = "runner";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Landing {
    Green,
    Running { run: String, group: i32, age: u64 },
    Failed { run: String, class: String, detail: String },
    Missing(String),
}

impl Landing {
    /// Why a head with this verdict may not go live, or none when its run ended green.
    pub(crate) fn refusal(&self) -> Option<String> {
        match self {
            Landing::Green => None,
            Landing::Running { run, group, age } => Some(format!("its after-landing run {run} is still running (process group {group}, started {age} s ago)")),
            Landing::Failed { run, class, detail } if detail.is_empty() => Some(format!("its after-landing run {run} ended {class}")),
            Landing::Failed { run, class, detail } => Some(format!("its after-landing run {run} ended {class}: {detail}")),
            Landing::Missing(why) => Some(why.clone()),
        }
    }
}

/// Whether the screen reads the after-landing verdict, from `after_landing_green`; only `off` turns it off.
pub(crate) fn enabled() -> bool {
    document(DEFAULTS)[SWITCH]["value"].as_str() != Some("off")
}

pub(crate) type AfterLanding = Box<dyn Fn(&str) -> Landing>;

#[derive(Deserialize)]
struct Runner {
    group: i32,
    started: f64,
    head: String,
    #[serde(default)]
    heads: Vec<String>,
}

pub(crate) struct Ledger {
    pub rows: PathBuf,
    pub logs: PathBuf,
    pub repository: String,
}

impl Ledger {
    /// The ledger and the lander queue of `checkout`'s repository on this host.
    pub(crate) fn here(checkout: &Path) -> Self {
        let origin = crate::platform::process::run("git", Some(checkout), &["remote", "get-url", "origin"]).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
        Self::of(&state_home(), &origin)
    }

    pub(crate) fn of(state_home: &Path, origin: &str) -> Self {
        Self { rows: state_home.join(LEDGER), logs: land_queue::directory(state_home, origin).join("logs"), repository: repository(origin) }
    }

    /// The verdict of `head` at `now` (seconds since the epoch).
    pub(crate) fn landing(&self, head: &str, now: u64) -> Landing {
        let Ok(text) = fs::read_to_string(&self.rows) else {
            return Landing::Missing(format!("{} has no after-landing run: this host keeps no after-landing ledger at {}", run_of(head), self.rows.display()));
        };
        // External format, written by the lander (north-landing post_land/execute.rs): post-land.tsv columns are at, repository, head, class, detail, lanes, seconds, covered_by, tests, mark, fresh.
        let rows: Vec<Vec<&str>> = text.lines().map(|line| line.split('\t').collect::<Vec<_>>()).filter(|fields| fields.get(1) == Some(&self.repository.as_str()) && fields.get(2) == Some(&head)).collect();
        let fresh = |row: &[&str]| row.get(10) == Some(&FRESH);
        let failed = |row: &[&str]| Landing::Failed { run: run_of(covering(row).unwrap_or(head)), class: row[3].to_owned(), detail: row.get(4).copied().unwrap_or_default().to_owned() };
        match rows.iter().rfind(|row| covering(row).is_none()) {
            Some(own) if own[3] != GREEN => return failed(own),
            Some(own) if fresh(own) => return Landing::Green,
            Some(_) => return Landing::Missing(format!("{} ended green but may have reused verdicts: its row in {} carries no fresh mark", run_of(head), self.rows.display())),
            None => {}
        }
        if let Some(row) = rows.iter().rfind(|row| fresh(row)) {
            return if row[3] == GREEN { Landing::Green } else { failed(row) };
        }
        if let Some((run, runner)) = self.runners().into_iter().find(|(_, runner)| runner.head == head || runner.heads.iter().any(|landed| landed == head)) {
            return Landing::Running { run, group: runner.group, age: now.saturating_sub(runner.started as u64) };
        }
        Landing::Missing(format!("{} never ran: {} holds no fresh after-landing row of {head} and no run of it is recorded as running", run_of(head), self.rows.display()))
    }

    fn runners(&self) -> Vec<(String, Runner)> {
        let read = |path: PathBuf| Some((path.file_stem()?.to_string_lossy().into_owned(), serde_json::from_str::<Runner>(&fs::read_to_string(&path).ok()?).ok()?));
        fs::read_dir(&self.logs).into_iter().flatten().flatten().map(|entry| entry.path()).filter(|path| path.extension().is_some_and(|extension| extension == RUNNER)).filter_map(read).collect()
    }
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

/// The repository name the lander writes in each row: the last path part of the origin URL, without `.git`.
fn repository(origin: &str) -> String {
    let name = origin.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(origin);
    name.strip_suffix(".git").unwrap_or(name).to_owned()
}

/// The batch tip whose run wrote `row` for another head, or none for a head's own run.
fn covering<'a>(row: &[&'a str]) -> Option<&'a str> {
    row.get(7).copied().filter(|tip| !tip.is_empty())
}

fn run_of(head: &str) -> String {
    format!("post-land-{}", &head[..head.len().min(12)])
}

#[cfg(test)]
mod tests;
