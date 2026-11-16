//! The capacity usage log: one JSON line per finished lease on this host, and the lander's own day files, which sizing, estimates and landing charges read back.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::estimate::{CapacityRun, task_signature};
use super::machine::{env, read_text};
use super::policy::{owner_prefix, sizing_runs};
use super::shape::command_shape;

pub const CENSOR_STALL_USEC: f64 = 1_000_000.0;
pub const CENSOR_AT_HIGH: f64 = 0.9;
pub const CENSOR_RAISE: f64 = 1.5;

fn state_root() -> PathBuf {
    env("XDG_STATE_HOME").map_or_else(|| Path::new(&env("HOME").unwrap_or_else(|| "/tmp".to_owned())).join(".local/state"), PathBuf::from)
}

pub fn usage_log_path() -> PathBuf {
    env("AGENT_CAPACITY_USAGE_LOG").map_or_else(|| state_root().join("agents/machine-capacity-usage.jsonl"), PathBuf::from)
}

fn private_parent(path: &Path) -> std::io::Result<()> {
    path.parent().map_or(Ok(()), |parent| fs::DirBuilder::new().recursive(true).mode(0o700).create(parent))
}

pub fn record_usage(entry: &str) -> std::io::Result<()> {
    let path = usage_log_path();
    private_parent(&path)?;
    let mut file = fs::OpenOptions::new().create(true).append(true).mode(0o600).open(&path)?;
    file.write_all(format!("{entry}\n").as_bytes())
}

/// Where the runs the lander's own leases made are kept, one file a day: under the state home's `north`, which no worker command writes, unlike the usage log, so only those leases set a landing command's charge.
fn lander_runs_directory() -> PathBuf {
    state_root().join("north/capacity/lander-runs")
}

fn day_file(at: &str) -> String {
    format!("{}.jsonl", at.get(..10).unwrap_or(at))
}

/// Appends `entry`, a run a lander's lease made at the ISO time `at`, to its day's file, and removes the files of the days before the ISO time `keep`.
pub fn record_lander_run(entry: &str, at: &str, keep: &str) -> std::io::Result<()> {
    let directory = lander_runs_directory();
    fs::DirBuilder::new().recursive(true).mode(0o700).create(&directory)?;
    let stale = day_file(keep);
    for old in fs::read_dir(&directory)?.flatten().filter(|file| *file.file_name().to_string_lossy() < *stale) {
        let _ = fs::remove_file(old.path());
    }
    let mut file = fs::OpenOptions::new().create(true).append(true).mode(0o600).open(directory.join(day_file(at)))?;
    file.write_all(format!("{entry}\n").as_bytes())
}

/// The lines the lander's leases recorded on the day of the ISO time `since` and after it.
pub fn lander_runs(since: &str) -> String {
    let first = day_file(since);
    let mut days: Vec<PathBuf> = fs::read_dir(lander_runs_directory()).into_iter().flatten().flatten().filter(|file| *file.file_name().to_string_lossy() >= *first).map(|file| file.path()).collect();
    days.sort();
    days.iter().filter_map(read_text).collect()
}

fn own_entries<'a>(lines: impl Iterator<Item = &'a str>) -> impl Iterator<Item = Value> {
    let host = crate::capacity::local_machine::host_name();
    lines.filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(move |entry| entry.get("host").and_then(Value::as_str).is_none_or(|recorded| recorded == host))
}

fn log_text() -> String {
    let text = read_text(usage_log_path()).unwrap_or_default();
    if env("AGENT_CAPACITY_USAGE_LOG").is_none() {
        north_base::platform::reader_stamp::touch(&state_root(), "agents/machine-capacity-usage.jsonl");
    }
    text
}

/// The latest sizing window of the usage log's last lines that match.
pub fn recent_usage(lines: usize, matches: impl Fn(&Value) -> bool) -> Vec<Value> {
    let text = log_text();
    let all: Vec<&str> = text.split('\n').collect();
    let matching: Vec<Value> = own_entries(all[all.len().saturating_sub(lines)..].iter().copied()).filter(|entry| matches(entry)).collect();
    let keep = matching.len().saturating_sub(sizing_runs());
    matching.into_iter().skip(keep).collect()
}

/// Every run this host's usage log holds, as estimates read them: a run that logged its directory and command is signed exactly, an older one by its shape alone.
pub fn history() -> Vec<CapacityRun> {
    own_entries(log_text().split('\n')).map(|entry| run_of(&entry)).collect()
}

fn run_of(entry: &Value) -> CapacityRun {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
    let real = |key: &str| entry.get(key).and_then(Value::as_f64);
    let command: Option<Vec<String>> = entry.get("command").and_then(Value::as_array).map(|words| words.iter().filter_map(Value::as_str).map(str::to_owned).collect());
    let cwd = text("cwd");
    let class = text("class").unwrap_or_default();
    let signature = match (&command, &cwd) {
        (Some(command), Some(cwd)) => task_signature(command, Some(cwd), &class, true),
        _ => task_signature(&text("shape").unwrap_or_default().split_whitespace().map(str::to_owned).collect::<Vec<_>>(), None, &class, false),
    };
    CapacityRun {
        at: text("at").unwrap_or_default(),
        signature,
        owner: text("owner"),
        shape: text("shape"),
        mean_cores: real("meanCores"),
        p90_cores: real("p90Cores"),
        peak_memory_mib: real("peakMemoryMiB"),
        io_read_bytes: real("ioReadBytes"),
        io_write_bytes: real("ioWriteBytes"),
        disk_busy_share: real("diskBusyShare"),
        wall_seconds: real("wallSeconds"),
        exit_code: real("exitCode"),
    }
}

pub fn command_key(owner: &str, command: &[String]) -> String {
    format!("{} {}", owner_prefix(owner), command_shape(command))
}

fn has_fingerprints(target: &Path) -> bool {
    let children = |dir: &Path| fs::read_dir(dir).into_iter().flatten().flatten().map(|entry| entry.path()).collect::<Vec<PathBuf>>();
    let nested = children(target).into_iter().flat_map(|child| children(&child));
    children(target).into_iter().chain(nested).any(|dir| fs::read_dir(dir.join(".fingerprint")).is_ok_and(|mut entries| entries.next().is_some()))
}

pub fn cache_state(cwd: &Path, target_dir: Option<&str>) -> &'static str {
    let target = target_dir.filter(|dir| !dir.is_empty()).map(|dir| cwd.join(dir)).or_else(|| cwd.ancestors().find(|dir| dir.join("Cargo.lock").is_file()).map(|dir| dir.join("target")));
    if target.is_some_and(|target| has_fingerprints(&target)) { "warm" } else { "cold" }
}

pub fn current_cache_state() -> &'static str {
    let cwd = std::env::current_dir().unwrap_or_default();
    cache_state(&cwd, env("CARGO_TARGET_DIR").as_deref())
}

pub fn censored(stall_usec: Option<f64>, peak_mib: f64, memory_high_mib: Option<f64>) -> bool {
    let at_high = memory_high_mib.is_some_and(|high| high.is_finite() && high > 0.0 && peak_mib >= high * CENSOR_AT_HIGH);
    stall_usec.is_some_and(|stall| stall > CENSOR_STALL_USEC) && at_high
}
