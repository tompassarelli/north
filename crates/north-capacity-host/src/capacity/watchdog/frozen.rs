use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use north_base::foundation::clock::Timestamp;
use north_capacity::capacity::agent_load::Host;

const STATE_FILE: &str = "frozen-scopes.json";
const HOLDS_FILE: &str = "gaming-frozen.json";
const SCOPE_PREFIX: &str = "north-session-";
const SCOPE_SUFFIX: &str = ".scope";
const SEARCH_DEPTH: usize = 4;
const UNHELD_LIMIT_MS: i64 = 5 * 60 * 1000;

struct Frozen {
    unit: String,
    path: PathBuf,
}

fn frozen_scopes(directory: &Path, depth: usize, found: &mut Vec<Frozen>) {
    for (name, path) in subdirectories(directory) {
        let scope = name.starts_with(SCOPE_PREFIX) && name.ends_with(SCOPE_SUFFIX);
        match (scope, is_frozen(&path)) {
            (true, true) => found.push(Frozen { unit: name, path }),
            (false, _) if depth < SEARCH_DEPTH => frozen_scopes(&path, depth + 1, found),
            _ => {}
        }
    }
}

fn subdirectories(directory: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(directory) else { return Vec::new() };
    entries.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| (entry.file_name().to_string_lossy().into_owned(), entry.path())).collect()
}

fn is_frozen(scope: &Path) -> bool {
    fs::read_to_string(scope.join("cgroup.freeze")).is_ok_and(|state| state.trim() == "1")
}

pub(super) fn sweep(cgroup: &Path, holds: &Path, dir: &Path, now_ms: i64) -> Vec<String> {
    let state_path = dir.join(STATE_FILE);
    let seen: BTreeMap<String, i64> = fs::read(&state_path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let held: BTreeSet<String> = fs::read(holds).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut next = BTreeMap::new();
    let mut thawed = Vec::new();
    let mut frozen = Vec::new();
    frozen_scopes(cgroup, 0, &mut frozen);
    for scope in frozen.into_iter().filter(|scope| !held.contains(&scope.unit)) {
        let since = seen.get(&scope.unit).copied().unwrap_or(now_ms);
        if now_ms - since <= UNHELD_LIMIT_MS {
            next.insert(scope.unit, since);
            continue;
        }
        let result = match fs::write(scope.path.join("cgroup.freeze"), "0") {
            Ok(()) => "thawed it".to_owned(),
            Err(error) => format!("could not thaw it: {error}"),
        };
        let at = Timestamp::from_unix_millis(now_ms).iso();
        record(&dir.join(super::INCIDENTS_FILE), &format!("- {at} frozen-session-scope: {} stayed frozen {} min with no recorded hold in {}; {result}\n", scope.unit, (now_ms - since) / 60_000, holds.display()));
        thawed.push(scope.unit);
    }
    let _ = fs::create_dir_all(dir);
    let _ = fs::write(&state_path, serde_json::to_vec(&next).unwrap_or_default());
    thawed
}

pub(super) fn sweep_live(dir: &Path, now_ms: i64) -> Vec<String> {
    let host = Host::from_env();
    sweep(&host.cgroup, &host.capacity.join(HOLDS_FILE), dir, now_ms)
}

fn record(path: &Path, entry: &str) {
    if let Err(error) = OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| file.write_all(entry.as_bytes())) {
        eprintln!("north capacity watchdog: {}: {error}", path.display());
    }
}
