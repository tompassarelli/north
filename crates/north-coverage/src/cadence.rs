use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::map;
use crate::run_selected::{north, repository};

pub const STALE_AFTER: Duration = Duration::from_secs(24 * 3600);
/// The share of the tests a lane's build lists that may be unknown to the map before a refresh starts.
pub const UNKNOWN_SHARE: usize = 10;

pub fn due(map: &Path, now: SystemTime) -> Option<String> {
    let Ok(written) = std::fs::metadata(map).and_then(|meta| meta.modified()) else { return Some("no map yet".to_owned()) };
    let age = now.duration_since(written).unwrap_or_default();
    (age >= STALE_AFTER).then(|| format!("the map is {} h old", age.as_secs() / 3600))
}

/// Why a lane's build starts a refresh: more than `UNKNOWN_SHARE` percent of the tests it lists are
/// unknown to the map. Exactly the share does not pass it.
pub fn unknown_reason(unknown: usize, listed: usize) -> Option<String> {
    (listed > 0 && unknown * 100 > listed * UNKNOWN_SHARE).then(|| format!("{unknown} of the {listed} tests the lane's build lists are unknown to the map, over {UNKNOWN_SHARE}%"))
}

pub fn run_start(arguments: &[String]) -> u8 {
    let (mut unknown, mut listed) = (None, None);
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--unknown", Some(value)) => match value.parse::<usize>() {
                Ok(count) => unknown = Some(count),
                Err(_) => return usage(),
            },
            ("--listed", Some(value)) => match value.parse::<usize>() {
                Ok(count) => listed = Some(count),
                Err(_) => return usage(),
            },
            _ => return usage(),
        }
    }
    let root = Path::new(".");
    let repository = repository(root);
    let reason = match (unknown, listed) {
        (Some(unknown), Some(listed)) => match unknown_reason(unknown, listed) {
            Some(reason) => reason,
            None => return 0,
        },
        (None, None) => match due(&map::path_for(&repository), SystemTime::now()) {
            Some(reason) => reason,
            None => return 0,
        },
        _ => return usage(),
    };
    match start(root, &reason) {
        Ok(started) => {
            println!("north test coverage: {started}");
            0
        }
        Err(why) => {
            eprintln!("north test coverage refresh: {why}");
            1
        }
    }
}

fn usage() -> u8 {
    eprintln!("usage: north test coverage refresh --start [--unknown N --listed M]");
    2
}

pub fn start(root: &Path, reason: &str) -> Result<String, String> {
    let repository = repository(root);
    let checkout = crate::refresh::checkout(&repository);
    if !checkout.is_dir() {
        return Err(format!("{repository} has no checkout at {}, where a refresh runs, so none starts ({reason})", checkout.display()));
    }
    let log = log_path(&repository);
    if let Some(directory) = log.parent() {
        std::fs::create_dir_all(directory).map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let unit = format!("north-coverage-refresh-{repository}");
    let output = Command::new("systemd-run")
        .args(["--user", "--quiet", "--collect", "--unit", &unit])
        .arg(format!("--property=StandardOutput=append:{}", log.display()))
        .arg(format!("--property=StandardError=append:{}", log.display()))
        .arg(format!("--setenv=PATH={}", path.to_string_lossy()))
        .arg(format!("--setenv=HOME={}", home.to_string_lossy()))
        .arg("--")
        .arg(north())
        .args(["test", "coverage", "refresh", "--repo", &repository])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("systemd-run: {error}"))?;
    if output.status.success() {
        Ok(format!("started a refresh of {repository} as {unit} ({reason}), logging to {}", log.display()))
    } else {
        Err(format!("cannot start {unit} ({reason}): {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// Tell this host's graph the map at `path`, then send it to every build host's graph and the
/// lander's; a host that cannot be reached keeps its older map and is named in the refresh's log.
pub fn ingest(root: &Path, path: &Path, repository: &str) {
    match Command::new(north()).args(["graph", "ingest", "coverage", "reads"]).stdout(Stdio::null()).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("north test coverage refresh: north graph ingest coverage reads exited {status}, so this host's graph keeps its older map"),
        Err(error) => eprintln!("north test coverage refresh: north graph ingest coverage reads: {error}"),
    }
    for warning in crate::hosts::tell(root, path, repository) {
        eprintln!("north test coverage refresh: {warning}");
    }
}

fn log_path(repository: &str) -> PathBuf {
    map::path_for(repository).with_extension("refresh.log")
}

#[cfg(test)]
#[path = "cadence_tests.rs"]
mod tests;
