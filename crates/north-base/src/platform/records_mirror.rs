use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::host;

pub const ROOT: &str = "north/records-mirror";
pub const STAMP: &str = "north/records-stamp.json";
pub const TICK_JOURNAL: &str = "north/fleet-tick.log";
pub const KEPT: Duration = Duration::from_secs(7 * 86_400);
pub const FRESH: Duration = Duration::from_secs(300);
const LEDGERS: [&str; 4] = ["north/enqueue-refusals.jsonl", "north/runtime-pins.jsonl", "safe-push/post-land.tsv", TICK_JOURNAL];
const TREES: [&str; 3] = ["north/land-queue", "north/gates", "north/decisions/placement"];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Stamp {
    pub host: String,
    pub state: PathBuf,
    pub at: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Records {
    home: PathBuf,
    from: Option<Stamp>,
}

impl Records {
    pub fn here() -> Self {
        Self::of(&state_home(), &host::name())
    }

    pub fn of(state: &Path, host: &str) -> Self {
        let root = state.join(ROOT);
        let from = fs::read_to_string(root.join(STAMP)).ok().and_then(|text| serde_json::from_str::<Stamp>(&text).ok()).filter(|stamp| stamp.host != host);
        match from {
            Some(stamp) => Self { home: root, from: Some(stamp) },
            None => Self { home: state.to_owned(), from: None },
        }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn mirrored(&self) -> bool {
        self.from.is_some()
    }

    pub fn source(&self) -> Option<&Stamp> {
        self.from.as_ref()
    }

    pub fn local(&self, path: &Path) -> PathBuf {
        match &self.from {
            Some(stamp) => path.strip_prefix(&stamp.state).map_or_else(|_| path.to_owned(), |rest| self.home.join(rest)),
            None => path.to_owned(),
        }
    }

    pub fn announce(&self, command: &str) {
        if let Some(note) = self.note(now()) {
            eprintln!("{command}: {note}");
        }
    }

    pub fn note(&self, now: f64) -> Option<String> {
        let stamp = self.from.as_ref()?;
        let age = (now - stamp.at).max(0.0);
        let age = if age < 120.0 { format!("{age:.0} s") } else { format!("{:.0} min", age / 60.0) };
        Some(format!("read from {}'s records as mirrored {age} ago into {}", stamp.host, self.home.display()))
    }
}

pub fn read(command: &str) -> Records {
    let records = Records::here();
    records.announce(command);
    records
}

pub fn selected(state: &Path, now: SystemTime) -> Vec<PathBuf> {
    let since = now.checked_sub(KEPT).unwrap_or(UNIX_EPOCH);
    let mut found: Vec<PathBuf> = LEDGERS.iter().map(PathBuf::from).filter(|path| state.join(path).is_file()).collect();
    for tree in TREES {
        walk(state, Path::new(tree), since, &mut found);
    }
    found
}

fn walk(state: &Path, relative: &Path, since: SystemTime, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(state.join(relative)).into_iter().flatten().flatten() {
        let (Ok(kind), Ok(metadata)) = (entry.file_type(), entry.metadata()) else { continue };
        let path = relative.join(entry.file_name());
        if kind.is_dir() {
            walk(state, &path, since, found);
        } else if kind.is_file() && path.extension().is_none_or(|extension| extension != "lock") && metadata.modified().is_ok_and(|modified| modified >= since) {
            found.push(path);
        }
    }
}

pub fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
#[path = "records_mirror_tests.rs"]
mod tests;
