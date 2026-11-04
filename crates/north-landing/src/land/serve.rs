use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::candidate::{self, Pick, git};
use super::after;
use super::bisect::{self, Judgement};
use super::checks::{self, Verdict};
use super::telemetry::Stages;
use super::{queue, red};

const USAGE: &str = "usage: north land serve [--once] [--repo DIR] [--checker PROGRAM]";
const ATTEMPTS: usize = 4;
const POLL: Duration = Duration::from_secs(10);

pub struct Lander {
    pub repo: PathBuf,
    pub worktree: PathBuf,
    pub queue: PathBuf,
    pub checker: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Round {
    Idle,
    Progress,
    Waiting(String),
}

pub(super) fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"))
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

impl Lander {
    pub fn new(repo: &Path, checker: Vec<String>) -> Result<Self, String> {
        let origin = git(repo, &["remote", "get-url", "--push", "origin"])?;
        Ok(Self { repo: repo.to_owned(), worktree: candidate::lander_worktree(repo)?, queue: queue::directory(&state_home(), &origin), checker })
    }

    fn identity(&self) -> Result<(String, String), String> {
        Ok((git(&self.worktree, &["rev-parse", "HEAD"])?, git(&self.worktree, &["rev-parse", "--absolute-git-dir"])?))
    }

    fn push(&self, candidate: &str, base: &str) -> Result<bool, String> {
        let pushed = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.worktree)
            .args(["push", "-q", "--no-verify", "--no-follow-tags", &format!("--force-with-lease=refs/heads/main:{base}"), "origin", &format!("{candidate}:refs/heads/main")])
            .env("SAFE_PUSH_ACTIVE", "1")
            .status()
            .map_err(|error| format!("git push: {error}"))?;
        if pushed.success() {
            return Ok(true);
        }
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        if now.split_whitespace().next() == Some(base) {
            return Err(format!("origin refused the push of {candidate} although main is still {base}"));
        }
        Ok(false)
    }

    fn base_moved(&self, base: &str) -> Result<bool, String> {
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        Ok(now.split_whitespace().next() != Some(base))
    }

    pub fn round(&self) -> Result<Round, String> {
        let logs = self.queue.join("logs");
        let mut limit = usize::MAX;
        let mut red_only = false;
        let mut rebuilds = 0;
        let mut progressed = false;
        after::recover(self)?;
        loop {
            let queued = queue::collect(&self.queue).map_err(|error| error.to_string())?;
            let tickets = if red_only { red::fixes(&queued) } else { queued };
            if tickets.is_empty() {
                return Ok(if progressed { Round::Progress } else { Round::Idle });
            }
            let base = match candidate::prepare(&self.repo, &self.worktree) {
                Ok(base) => base,
                Err(error) => return Ok(Round::Waiting(format!("transient: {error}"))),
            };
            let built = candidate::build(&self.worktree, &base, &tickets, limit, &logs)?;
            let mut included = Vec::new();
            let mut undecided = None;
            for ((path, ticket), pick) in tickets.iter().zip(&built.picks) {
                match pick {
                    Pick::Included => included.push((path, ticket)),
                    Pick::Decided(verdict) => {
                        bisect::decide(path, ticket, verdict.clone())?;
                        progressed = true;
                    }
                    Pick::Undecided(reason) => undecided = Some(reason.clone()),
                }
            }
            if included.is_empty() {
                return Ok(match (progressed, undecided) {
                    (true, _) => Round::Progress,
                    (false, Some(reason)) => Round::Waiting(reason),
                    (false, None) => Round::Idle,
                });
            }
            let identity = self.identity()?;
            println!("north land: candidate {} carries {} lanes onto main@{base}; checking it", built.head, built.included());
            let log = logs.join(format!("{}-candidate.log", micros()));
            let mut stages = Stages::start();
            let verdict = checks::run(&self.checker, &self.worktree, &base, &log);
            stages.mark("check");
            if (self.identity()? != identity || identity.0 != built.head) && !matches!(verdict, Verdict::Refused(_)) {
                return Ok(Round::Waiting(format!("the lander worktree moved while candidate {} was checked; nothing pushed", built.head)));
            }
            let moved = match &verdict {
                Verdict::Green => false,
                _ => match self.base_moved(&base) {
                    Ok(moved) => moved,
                    Err(error) => return Ok(Round::Waiting(format!("transient: {error}"))),
                },
            };
            match bisect::judge(verdict, included.len(), moved) {
                Judgement::Land => {
                    after::begin(self, &built.head, &base, &included)?;
                    if !self.push(&built.head, &base)? {
                        after::abandon(self);
                        println!("north land: main moved from {base} before the push; rebuilding the candidate");
                        rebuilds += 1;
                        if rebuilds >= ATTEMPTS {
                            break;
                        }
                        continue;
                    }
                    stages.mark("push");
                    after::finish(self, &built.head, &base, &stages, included.into_iter().map(|(path, ticket)| (path.clone(), ticket.clone())).collect())?;
                    return Ok(Round::Progress);
                }
                Judgement::Rebuild => {
                    println!("north land: main moved from {base} while candidate {} was checked; rebuilding it, no lane judged", built.head);
                    limit = usize::MAX;
                    rebuilds += 1;
                    if rebuilds >= ATTEMPTS {
                        break;
                    }
                }
                Judgement::Halve(half) => {
                    println!("north land: candidate {} refused; checking its first {half} lanes", built.head);
                    limit = half;
                }
                Judgement::Culprit(detail) => {
                    let (path, ticket) = included[0];
                    bisect::decide(path, ticket, format!("failed {detail}"))?;
                    return Ok(Round::Progress);
                }
                Judgement::RedMain(detail) if !red_only && red::narrows(&included) => {
                    println!("north land: main is red ({detail}); checking the red-fix lanes alone first");
                    red_only = true;
                    limit = usize::MAX;
                }
                Judgement::RedMain(detail) => return Ok(Round::Waiting(red::held(&detail, included.len()))),
                Judgement::Hold(reason) => return Ok(Round::Waiting(reason)),
            }
        }
        Ok(if progressed { Round::Progress } else { Round::Waiting(format!("no candidate settled in {ATTEMPTS} attempts")) })
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let mut once = false;
    let mut repo = PathBuf::from(".");
    let mut checker = vec!["safe-push".to_owned()];
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.clone().next()) {
            ("--once", _) => once = true,
            ("--repo", Some(value)) => {
                repo = PathBuf::from(value);
                rest.next();
            }
            ("--checker", Some(value)) => {
                checker = vec![value.clone()];
                rest.next();
            }
            ("-h" | "--help", _) => {
                println!("{USAGE}");
                return 0;
            }
            _ => {
                eprintln!("{USAGE}");
                return 64;
            }
        }
    }
    let lander = match Lander::new(&repo, checker) {
        Ok(lander) => lander,
        Err(error) => {
            eprintln!("north land serve: {error}");
            return 1;
        }
    };
    loop {
        match lander.round() {
            Ok(Round::Progress) => continue,
            Ok(Round::Idle) if once => return 0,
            Ok(Round::Waiting(reason)) if once => {
                println!("north land: waiting: {reason}");
                return 75;
            }
            Ok(Round::Waiting(reason)) => println!("north land: waiting: {reason}"),
            Ok(Round::Idle) => {}
            Err(error) if once => {
                eprintln!("north land serve: {error}");
                return 1;
            }
            Err(error) => eprintln!("north land serve: {error}"),
        }
        std::thread::sleep(POLL);
    }
}
