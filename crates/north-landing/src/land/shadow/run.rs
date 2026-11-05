use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::super::candidate::{self, Pick};
use super::super::checks::{ROUND, SCRUBBED};
use super::super::queue::Ticket;
use super::super::setting;
use super::super::slots::select::Config;
use super::super::reuse::{self, Key, Verdict};
use super::{OWNER, Settings};

const INFRASTRUCTURE: i32 = 75;
const RED_MAIN: i32 = 3;
const SLOTS_BUSY: i32 = 96;
const TIMED_OUT: i32 = 124;
const CANCELLED: i32 = 130;
const KILLED: i32 = 137;

pub struct Found {
    pub head: String,
    pub lines: Vec<(String, Key)>,
}

pub fn missing(settings: &Settings, worktree: &Path, main: &str, path: &Path, ticket: &Ticket, logs: &Path) -> Option<Found> {
    let built = candidate::build(worktree, main, &[(path.to_owned(), ticket.clone())], 1, logs).ok()?;
    if !matches!(built.picks.as_slice(), [Pick::Included]) {
        return None;
    }
    let commands = Config::load(worktree, &built.head).commands(worktree, main, &built.head).ok()?;
    let lines: Vec<(String, Key)> = commands
        .into_iter()
        .filter_map(|line| {
            let input = reuse::input_hash(worktree, &line).ok()?;
            let key = Key { lane_head: ticket.head.clone(), main: main.to_owned(), input };
            reuse::lookup(&settings.verdicts, &key).is_none().then_some((line, key))
        })
        .collect();
    (!lines.is_empty()).then_some(Found { head: built.head, lines })
}

pub fn verdict_of(code: Option<i32>) -> Option<Verdict> {
    match code {
        Some(0) => Some(Verdict::Pass),
        Some(INFRASTRUCTURE | RED_MAIN | SLOTS_BUSY | TIMED_OUT | CANCELLED | KILLED) | None => None,
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
        let Some((_, log)) = log_for(logs, &key.lane_head, index) else {
            unrecorded += 1;
            continue;
        };
        let Ok(errors) = log.try_clone() else {
            unrecorded += 1;
            continue;
        };
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]).current_dir(worktree).stdin(Stdio::null()).stdout(log).stderr(errors).env("NORTH_LAND_SERVING", "1").env("NORTH_OFFLOAD_OWNER", OWNER).env(ROUND, OWNER);
        for name in SCRUBBED {
            command.env_remove(name);
        }
        match command.status().ok().and_then(|status| verdict_of(status.code())) {
            Some(verdict) => {
                let _ = reuse::record(&settings.verdicts, key, verdict);
                recorded += 1;
            }
            None => unrecorded += 1,
        }
    }
    (recorded, unrecorded)
}
