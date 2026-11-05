mod config;
mod judge;
#[cfg(test)]
mod tests;

use std::env;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::platform::github::{CommitStatuses, Gh};
use config::{Check, Step};
use judge::{Finished, Outcome, Overrides, Places, StatusRequest, Statuses, Steps};

const SOURCE: &str = include_str!("../../../../../north-data/private-checks.toml");
const USAGE: &str = "usage: north private-check [--config PATH] [--check NAME] [--dry-run [--ref REV] [--since REV]]

Runs each [[check]] of north-data/private-checks.toml (built in; --config reads another file): repo, checkout under ~/code, status context, git pathspec paths and [[check.step]] tables (command, dir, machine-capacity class, timeout_seconds).
A check posts pending, then success when the last step exits 0, failure on exit 1 and error otherwise; a step capacity never admitted (exit 75) posts nothing, records nothing and makes the run exit 75.
--check NAME   run only that check
--dry-run      run the steps but print each status request instead of posting, and record nothing
--ref REV      (dry run) judge REV instead of origin/main
--since REV    (dry run) diff from REV instead of the last judged tip";

#[derive(Debug, Default)]
struct Request {
    config: Option<PathBuf>,
    check: Option<String>,
    overrides: Overrides,
}

fn parse(arguments: &[String]) -> Result<Request, String> {
    let mut request = Request::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let mut value = || words.next().cloned().ok_or_else(|| format!("{word} needs a value"));
        match word.as_str() {
            "--config" => request.config = Some(PathBuf::from(value()?)),
            "--check" => request.check = Some(value()?),
            "--ref" => request.overrides.reference = Some(value()?),
            "--since" => request.overrides.since = Some(value()?),
            "--dry-run" => request.overrides.dry_run = true,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let pinned = request.overrides.reference.is_some() || request.overrides.since.is_some();
    if pinned && !request.overrides.dry_run {
        return Err("--ref and --since judge a chosen commit, so they need --dry-run".into());
    }
    Ok(request)
}

pub fn capacity_argv(step: &Step, owner: &str) -> Vec<String> {
    let mut argv = ["capacity", "host"].map(str::to_owned).to_vec();
    let timeout = step.timeout_seconds.to_string();
    argv.extend(["run", "--class", &step.class, "--owner", owner, "--timeout-seconds", &timeout, "--"].map(str::to_owned));
    argv.extend(step.command.iter().cloned());
    argv
}

struct Admitted {
    north: PathBuf,
}

impl Steps for Admitted {
    fn run(&mut self, dir: &Path, step: &Step, owner: &str) -> Result<Finished, String> {
        let argv = capacity_argv(step, owner);
        println!("private-check: in {}: north {}", dir.display(), argv.join(" "));
        let mut child = Command::new(&self.north).args(&argv).current_dir(dir).stdout(Stdio::piped()).spawn().map_err(|error| format!("cannot start north: {error}"))?;
        let mut last_line = String::new();
        for line in BufReader::new(child.stdout.take().ok_or("no stdout")?).lines() {
            let line = line.map_err(|error| format!("reading output: {error}"))?;
            println!("{line}");
            if !line.trim().is_empty() {
                last_line = line.trim().to_owned();
            }
        }
        let status = child.wait().map_err(|error| format!("waiting on north: {error}"))?;
        Ok(Finished { code: status.code().unwrap_or(-1), last_line })
    }
}

struct Posted;

impl Statuses for Posted {
    fn post(&mut self, request: &StatusRequest) -> Result<(), String> {
        Gh.post_status(&request.repo, &request.sha, &request.context, request.state, &request.description).map_err(|error| format!("posting {} on {}: {error:?}", request.state, request.sha))?;
        println!("private-check: posted {} {} on {}: {}", request.context, request.state, request.sha, request.description);
        Ok(())
    }
}

struct Printed;

impl Statuses for Printed {
    fn post(&mut self, request: &StatusRequest) -> Result<(), String> {
        println!("private-check: would POST https://api.github.com/repos/{}/statuses/{} {}", request.repo, request.sha, request.body());
        Ok(())
    }
}

fn places() -> Places {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let host = fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_else(|_| "this host".into());
    Places { code: home.join("code"), state: home.join(".local/state/north/private-check"), host }
}

fn locked(places: &Places, check: &Check) -> Result<Option<File>, String> {
    fs::create_dir_all(&places.state).map_err(|error| format!("cannot create {}: {error}", places.state.display()))?;
    let path = places.state.join(format!("{}.lock", check.name));
    let file = File::create(&path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(Some(file)),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
        Err(error) => Err(format!("cannot lock {}: {error}", path.display())),
    }
}

fn report(check: &Check, outcome: Result<Outcome, String>) -> u8 {
    match outcome {
        Ok(Outcome::Current(tip)) => println!("private-check: {}: {tip} is already judged", check.name),
        Ok(Outcome::Untouched(tip)) => println!("private-check: {}: nothing under its paths changed up to {tip}; recorded", check.name),
        Ok(Outcome::Deferred(tip)) => {
            println!("private-check: {}: machine-capacity did not admit a step of {tip} before its timeout; left unjudged for the next run", check.name);
            return 75;
        }
        Ok(Outcome::Judged(last)) => println!("private-check: {}: {} {} on {}", check.name, last.state, last.description, last.sha),
        Err(why) => {
            eprintln!("private-check: {}: {why}", check.name);
            return 1;
        }
    }
    0
}

fn judge_one(places: &Places, check: &Check, request: &Request) -> u8 {
    let _lock = match locked(places, check) {
        Ok(Some(lock)) => lock,
        Ok(None) => {
            println!("private-check: {}: another run holds its lock; skipped", check.name);
            return 0;
        }
        Err(why) => return report(check, Err(why)),
    };
    let mut runner = Admitted { north: north_base::platform::north_binary::north_binary() };
    let statuses: &mut dyn Statuses = if request.overrides.dry_run { &mut Printed } else { &mut Posted };
    report(check, judge::judge(places, check, &request.overrides, &mut runner, statuses))
}

pub fn run(arguments: &[String]) -> u8 {
    let request = match parse(arguments) {
        Ok(request) => request,
        Err(why) => {
            eprintln!("private-check: {why}\n{USAGE}");
            return 2;
        }
    };
    let source = match &request.config {
        Some(path) => fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display())),
        None => Ok(SOURCE.to_owned()),
    };
    let checks = match source.and_then(|source| config::parse(&source)) {
        Ok(checks) => checks,
        Err(why) => {
            eprintln!("private-check: {why}");
            return 2;
        }
    };
    let chosen: Vec<&Check> = checks.iter().filter(|check| request.check.as_ref().is_none_or(|name| *name == check.name)).collect();
    if let Some(name) = request.check.as_ref().filter(|_| chosen.is_empty()) {
        eprintln!("private-check: no check named {name}");
        return 2;
    }
    let places = places();
    chosen.into_iter().map(|check| judge_one(&places, check, &request)).max().unwrap_or(0)
}
