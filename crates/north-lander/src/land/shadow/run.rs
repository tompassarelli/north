use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::candidate::{self, Pick};
use super::super::checks::{ROUND, SCRUBBED};
use super::super::queue::Ticket;
use super::super::setting;
use super::super::slots::select::Config;
use super::super::slots::unit_reuse;
use super::super::slots::verdict_key;
use super::{OWNER, Settings};
use north_base::foundation::exit::{CANCELLED_STATUS, KILLED_STATUS, NO_VERDICT_STATUS, RED_MAIN_STATUS, SLOTS_BUSY_STATUS, TIMED_OUT_STATUS};
use north_base::foundation::offload::OWNER_ENV;


pub struct Found {
    pub head: String,
    pub lines: Vec<(String, String)>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Pass,
    Fail,
}

fn north(settings: &Settings, worktree: &Path, arguments: &[String]) -> Option<std::process::Output> {
    let (program, prefix) = settings.north.split_first()?;
    Command::new(program).args(prefix).args(arguments).current_dir(worktree).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()
}

fn failed_marker(logs: &Path, key: &str) -> PathBuf {
    logs.join(format!("{key}.failed"))
}

pub fn missing(settings: &Settings, worktree: &Path, main: &str, path: &Path, ticket: &Ticket, logs: &Path) -> Option<Found> {
    let built = candidate::build(worktree, main, &[(path.to_owned(), ticket.clone())], &[], 1, logs).ok()?;
    if !matches!(built.picks.as_slice(), [Pick::Included]) {
        return None;
    }
    let commands = Config::load(worktree, &built.head).commands(worktree, main, &built.head).ok()?;
    let environment = verdict_key::lane_environment(worktree, main, &built.head, &commands);
    let keyed: Vec<(String, String)> = commands.into_iter().filter_map(|line| Some((line.clone(), verdict_key::key(worktree, main, &built.head, &line, &environment)?))).collect();
    if keyed.is_empty() {
        return None;
    }
    let keys: Vec<&str> = keyed.iter().map(|(_, key)| key.as_str()).collect();
    let output = north(settings, worktree, &verdict_key::hit_arguments(&keys)).filter(|output| output.status.success())?;
    let held = verdict_key::hits(&String::from_utf8_lossy(&output.stdout));
    let lines: Vec<(String, String)> = keyed.into_iter().filter(|(_, key)| !held.contains_key(key) && !failed_marker(logs, key).exists()).collect();
    (!lines.is_empty()).then_some(Found { head: built.head, lines })
}

pub fn verdict_of(code: Option<i32>) -> Option<Verdict> {
    match code {
        Some(0) => Some(Verdict::Pass),
        Some(NO_VERDICT_STATUS | RED_MAIN_STATUS | SLOTS_BUSY_STATUS | TIMED_OUT_STATUS | CANCELLED_STATUS | KILLED_STATUS) | None => None,
        Some(_) => Some(Verdict::Fail),
    }
}

fn wrapped(capacity: &str, argv: Vec<String>) -> Vec<String> {
    if capacity.is_empty() {
        return argv;
    }
    let mut command = vec!["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), format!("exec {capacity} \"$@\""), "north-land-shadow-capacity".to_owned()];
    command.extend(argv);
    command
}

fn log_for(logs: &Path, head: &str, index: usize) -> Option<(PathBuf, File)> {
    fs::create_dir_all(logs).ok()?;
    let path = logs.join(format!("{}.{index}.log", &head[..head.len().min(12)]));
    File::create(&path).ok().map(|file| (path, file))
}

pub fn check(settings: &Settings, worktree: &Path, main: &str, host: &str, found: &Found, logs: &Path) -> (usize, usize) {
    let capacity = setting::landing(&settings.repo, "refs/remotes/origin/main", "landing.capacity");
    let (mut recorded, mut unrecorded) = (0, 0);
    for (index, (line, key)) in found.lines.iter().enumerate() {
        let mut argv = settings.checker.clone();
        argv.extend(["--leased", "--base", main, "--head", &found.head, "--runner-host", host, "--command", line].map(str::to_owned));
        let argv = wrapped(&capacity, argv);
        let Some((_, log)) = log_for(logs, &found.head, index) else {
            unrecorded += 1;
            continue;
        };
        let Ok(errors) = log.try_clone() else {
            unrecorded += 1;
            continue;
        };
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]).current_dir(worktree).stdin(Stdio::null()).stdout(log).stderr(errors).env("NORTH_LAND_SERVING", "1").env(unit_reuse::RECORD_ENV, "1").env(OWNER_ENV, OWNER).env(ROUND, OWNER);
        for name in SCRUBBED {
            command.env_remove(name);
        }
        match command.status().ok().and_then(|status| verdict_of(status.code())) {
            Some(verdict) => {
                let passed = (verdict == Verdict::Pass).then_some(found.head.as_str());
                let written = north(settings, worktree, &verdict_key::record_arguments(host, key, passed)).is_some_and(|output| output.status.success());
                if verdict == Verdict::Fail {
                    let _ = fs::write(failed_marker(logs, key), "");
                }
                if written { recorded += 1 } else { unrecorded += 1 }
            }
            None => unrecorded += 1,
        }
    }
    (recorded, unrecorded)
}
