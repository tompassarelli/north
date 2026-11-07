use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

use super::affected::Scope;
use crate::land::lander::state_home;

const KEEP: Duration = Duration::from_secs(2 * 24 * 60 * 60);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recorded {
    pub scope: Option<Scope>,
    pub it: Option<String>,
}

fn directory() -> PathBuf {
    state_home().join("safe-push").join("selections")
}

pub fn load(head: &str) -> Option<Recorded> {
    serde_json::from_str(&fs::read_to_string(directory().join(head)).ok()?).ok()
}

pub fn save(head: &str, selection: Recorded) {
    if selection == Recorded::default() {
        return;
    }
    let earlier = load(head).unwrap_or_default();
    let merged = Recorded { scope: selection.scope.or(earlier.scope), it: selection.it.or(earlier.it) };
    let directory = directory();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    if let Ok(entries) = fs::read_dir(&directory) {
        for entry in entries.flatten() {
            let stale = entry.metadata().and_then(|metadata| metadata.modified()).ok().and_then(|modified| SystemTime::now().duration_since(modified).ok()).is_some_and(|age| age > KEEP);
            if stale {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    if let Ok(text) = serde_json::to_string(&merged) {
        let _ = fs::write(directory.join(head), text);
    }
}
