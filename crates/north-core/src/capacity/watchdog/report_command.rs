use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_base::foundation::clock::Timestamp;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

use crate::ops::provider_capacity::Outcome;
use super::report::{Inputs, Run, handoff_fix, parse_time, report};
use super::sampler::{ROTATED_FILE, SAMPLES_FILE};
use super::snapshot::read;

const DEFAULT_WINDOWS: [f64; 2] = [5.0, 30.0];
const KEPT_SAMPLES: usize = 200;

fn lines(path: &Path) -> Vec<String> {
    read(path).unwrap_or_default().lines().filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()))
}

fn ledger_path(dir: &Path, north: &Path) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("THREADS_DB") {
        return Ok(PathBuf::from(path));
    }
    let remote = home().join(".config/threads/remote");
    let Some(host) = read(&remote).map(|text| text.trim().to_owned()) else { return Ok(home().join(".local/state/threads/threads.db")) };
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let snapshot = dir.join("ledger-snapshot.db");
    let pulled = Command::new(north).args(["threads", "snapshot-pull", "--force", "--snapshot"]).arg(&snapshot).arg(host).stdin(Stdio::null()).stderr(Stdio::piped()).output().map_err(|error| error.to_string())?;
    if !pulled.status.success() {
        return Err(format!("threads snapshot-pull exit {}", pulled.status.code().map_or_else(|| "signal".to_owned(), |code| code.to_string())));
    }
    Ok(snapshot)
}

fn runs(path: &Path, since: i64, now: i64) -> Result<Vec<Run>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| error.to_string())?;
    let mut statement = db
        .prepare("SELECT agent, tier, role, ended, peak_ctx, handoff_fires, handoff_ignored FROM runs WHERE julianday(ended) >= julianday(?) AND julianday(ended) <= julianday(?)")
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([Timestamp::from_unix_millis(since).iso_millis(), Timestamp::from_unix_millis(now).iso_millis()], |row| {
            Ok(Run { agent: row.get(0)?, tier: row.get(1)?, role: row.get(2)?, ended: row.get(3)?, peak_ctx: row.get(4)?, handoff_fires: row.get(5)?, handoff_ignored: row.get(6)? })
        })
        .map_err(|error| error.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|error| error.to_string())
}

pub(super) fn capacity_flag(hits: Result<Vec<Outcome>, String>) -> Option<String> {
    let hits = match hits {
        Err(error) => return Some(format!("provider-capacity check unavailable: {error}")),
        Ok(hits) if hits.is_empty() => return None,
        Ok(hits) => hits,
    };
    let mut models: Vec<&str> = Vec::new();
    for hit in &hits {
        let model = hit.model.as_deref().unwrap_or_default();
        if !models.contains(&model) {
            models.push(model);
        }
    }
    Some(format!("provider-capacity codex: {} turn(s) of {} at capacity, last {}", hits.len(), models.join(", "), hits[hits.len() - 1].at))
}

pub(super) fn run(arguments: &[String]) -> Result<(), String> {
    let (mut minutes, mut ledger_since) = (Vec::new(), None);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--minutes" => minutes.push(words.next().and_then(|text| text.parse::<f64>().ok()).ok_or(super::USAGE)?),
            "--since" => ledger_since = Some(words.next().and_then(|text| parse_time(text)).ok_or("invalid --since date")?),
            _ => return Err(super::USAGE.to_owned()),
        }
    }
    let dir = super::directory();
    let samples: Vec<Value> = {
        let all: Vec<String> = lines(&dir.join(ROTATED_FILE)).into_iter().chain(lines(&dir.join(SAMPLES_FILE))).collect();
        all[all.len().saturating_sub(KEPT_SAMPLES)..].iter().filter_map(|line| serde_json::from_str(line).ok()).collect()
    };
    let incidents: Vec<String> = lines(&dir.join(super::INCIDENTS_FILE)).into_iter().filter(|line| line.starts_with("- ")).collect();
    let windows = if minutes.is_empty() { DEFAULT_WINDOWS.to_vec() } else { minutes };
    let now = Timestamp::now().unix_millis();
    let widest = windows.iter().copied().fold(f64::MIN, f64::max);
    let since = ledger_since.unwrap_or_else(|| (now - (widest * 60000.0) as i64).max(handoff_fix()));
    let north = super::north_binary();
    let ledger = ledger_path(&dir, &north).and_then(|path| runs(&path, since, now));
    let (ledger_runs, ledger_error) = match ledger {
        Ok(found) => (found, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    let handoff_ctx = super::detect::registry("handoff_ctx");
    for window in windows {
        let inputs = Inputs { samples: &samples, incidents: &incidents, runs: &ledger_runs, ledger_since, handoff_ctx, now };
        let mut result = report(&inputs, window);
        let flags = result["flags"].as_array_mut().expect("a report has flags");
        flags.extend(ledger_error.iter().map(|error| Value::String(format!("handoff check unavailable: {error}"))));
        flags.extend(capacity_flag(crate::ops::provider_capacity::capacity_events(window)).map(Value::String));
        println!("{result}");
    }
    Ok(())
}
