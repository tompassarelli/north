use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::after::follow::program;
use super::{candidate, hold, lander, queue, repositories};

mod coverage;
mod main_run;
mod pick;
mod reuse;
mod run;

const USAGE: &str = "usage: north land shadow tick [--repo DIR] (without --repo, every repository the lander serves)";
const WORKTREE: &str = "shadow";
const LOCK: &str = "north/land-shadow.lock";
pub const OWNER: &str = "north-land-shadow";

#[derive(Debug, PartialEq, Eq)]
pub enum Round {
    Locked,
    Idle,
    NoRoom(String),
    Ran { branch: String, recorded: usize, unrecorded: usize },
    RecordedMain { main: String, lines: usize },
}

#[derive(Clone)]
pub struct Settings {
    pub repo: PathBuf,
    pub state: PathBuf,
    pub north: Vec<String>,
    pub queue: PathBuf,
    pub host: String,
    pub checker: Vec<String>,
    pub chooser: Vec<String>,
}

impl Settings {
    fn of(repo: &Path) -> Result<Self, String> {
        let origin = candidate::git(repo, &["remote", "get-url", "--push", "origin"])?;
        let state = lander::state_home();
        Ok(Self {
            repo: repo.to_owned(),
            queue: queue::directory(&state, &origin),
            north: vec![program()],
            state,
            host: repositories::local_host(),
            checker: vec![program(), "land".to_owned(), "checks".to_owned()],
            chooser: vec![program(), "capacity".to_owned(), "offload".to_owned(), "choose".to_owned(), "--any".to_owned()],
        })
    }
}

fn lock(state: &Path) -> Result<Option<hold::Locked>, String> {
    let path = state.join(LOCK);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let file = File::options().create(true).append(true).open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(hold::try_lock(file))
}

fn place(chooser: &[String]) -> Result<String, String> {
    let Some((chooser, arguments)) = chooser.split_first() else {
        return Err("no host chooser configured".to_owned());
    };
    let output = Command::new(chooser).args(arguments).stdin(std::process::Stdio::null()).output().map_err(|error| format!("{chooser}: {error}"))?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()) }
}

pub fn tick(settings: &Settings) -> Result<Round, String> {
    let Some(_held) = lock(&settings.state)? else {
        return Ok(Round::Locked);
    };
    let worktree = candidate::lander_worktree(&settings.repo, WORKTREE)?;
    let main = candidate::prepare(&settings.repo, &worktree)?;
    let logs = settings.state.join("north/land-shadow");
    let tickets = queue::pending(&settings.queue).map_err(|error| format!("{}: {error}", settings.queue.display()))?;
    let queued = pick::not_checking(pick::ordered(tickets, &settings.host), &settings.queue);
    let host = if queued.is_empty() {
        String::new()
    } else {
        match place(&settings.chooser) {
            Ok(host) => host,
            Err(reason) => return Ok(Round::NoRoom(reason)),
        }
    };
    let Some(work) = pick::first_with_work(queued, |path, ticket| run::missing(settings, &worktree, &main, &host, path, ticket, &logs)) else {
        if main_run::pending(&logs, &main) && let Some(lines) = main_run::record(settings, &worktree, &main, &logs) {
            return Ok(Round::RecordedMain { main, lines });
        }
        if let Some(line) = coverage::tend(settings) {
            println!("north land shadow: {}: {line}", settings.repo.display());
        }
        return Ok(Round::Idle);
    };
    let (recorded, unrecorded) = run::check(settings, &worktree, &main, &host, &work.found, &logs);
    Ok(Round::Ran { branch: work.ticket.branch, recorded, unrecorded })
}

pub fn sweep(settings: &[Settings]) -> Vec<Result<Round, String>> {
    let mut rounds = Vec::new();
    for settings in settings {
        let round = tick(settings);
        let finished = !matches!(round, Ok(Round::Idle) | Err(_));
        rounds.push(round);
        if finished {
            break;
        }
    }
    rounds
}

fn say(repo: &Path, round: &Result<Round, String>) -> bool {
    let name = repo.display();
    match round {
        Ok(Round::Locked) => println!("north land shadow: {name}: another tick holds the lock"),
        Ok(Round::Idle) => println!("north land shadow: {name}: every queued lane has its verdicts"),
        Ok(Round::NoRoom(reason)) => println!("north land shadow: {name}: no host has room, skipped the round: {reason}"),
        Ok(Round::RecordedMain { main, lines }) => println!("north land shadow: {name}: recorded the nextest land check of main {main} in the background slot, {lines} line(s)"),
        Ok(Round::Ran { branch, recorded, unrecorded }) => println!("north land shadow: {name}: {branch}: recorded {recorded} verdict(s), {unrecorded} without a verdict"),
        Err(error) => {
            eprintln!("north land shadow: {name}: {error}");
            return false;
        }
    }
    true
}

pub use reuse::run as reuse_run;

pub fn run(arguments: &[String]) -> u8 {
    let repos = match arguments {
        [tick] if tick == "tick" => repositories::discovered(&lander::state_home(), &repositories::local_host()).checkouts,
        [tick, flag, dir] if tick == "tick" && flag == "--repo" => vec![PathBuf::from(dir)],
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    let mut settings = Vec::new();
    let mut ok = true;
    for repo in &repos {
        match Settings::of(repo) {
            Ok(found) => settings.push(found),
            Err(error) => {
                eprintln!("north land shadow: {}: {error}", repo.display());
                ok = false;
            }
        }
    }
    for (found, round) in settings.iter().zip(sweep(&settings)) {
        ok &= say(&found.repo, &round);
    }
    u8::from(!ok)
}

#[cfg(test)]
mod tests;
