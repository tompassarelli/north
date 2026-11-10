use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::map;
use crate::run_selected::{north, repository};

pub const STALE_AFTER: Duration = Duration::from_secs(24 * 3600);

pub fn due(map: &Path, now: SystemTime) -> Option<String> {
    let Ok(written) = std::fs::metadata(map).and_then(|meta| meta.modified()) else { return Some("no map yet".to_owned()) };
    let age = now.duration_since(written).unwrap_or_default();
    (age >= STALE_AFTER).then(|| format!("the map is {} h old", age.as_secs() / 3600))
}

pub fn run_start() -> u8 {
    let root = Path::new(".");
    let repository = repository(root);
    let Some(reason) = due(&map::path_for(&repository), SystemTime::now()) else {
        println!("north test coverage: the map of {repository} is under {} h old", STALE_AFTER.as_secs() / 3600);
        return 0;
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

pub fn start(root: &Path, reason: &str) -> Result<String, String> {
    let repository = repository(root);
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

fn log_path(repository: &str) -> PathBuf {
    map::path_for(repository).with_extension("refresh.log")
}

#[cfg(test)]
#[path = "cadence_tests.rs"]
mod tests;
