use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::candidate::{self, Pick, git};
use super::lander::{self, Lander, state_home};
use super::after;
use super::bisect::{self, Judgement};
use super::culprit::{self, Found};
use super::checks::Verdict;
use super::credential;
use super::telemetry::Stages;
use super::push_refusal::{self, Backoff, Refusal};
use super::records::{self, Candidate, Decision};
use super::{queue, red, sheriff, speculate};
use super::repositories;

const USAGE: &str = "usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]";
const ATTEMPTS: usize = 4;
const POLL: Duration = Duration::from_secs(2);
const DISCOVERY: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
pub enum Round {
    Idle,
    Progress,
    Waiting(String),
    PushRefused(String),
}

pub(super) enum Pushed {
    Landed,
    Moved,
    Refused(Refusal),
}

fn runtime_link() -> PathBuf {
    std::env::var_os("NORTH_RUNTIME_LINK").filter(|value| !value.is_empty()).map_or_else(|| state_home().join("north/current"), PathBuf::from)
}

fn runtime_target(link: &Path) -> Option<PathBuf> {
    std::fs::read_link(link).ok()
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

impl Lander {
    fn identity(&self) -> Result<(String, String), String> {
        Ok((git(&self.worktree, &["rev-parse", "HEAD"])?, git(&self.worktree, &["rev-parse", "--absolute-git-dir"])?))
    }

    pub(super) fn push_token(&self, base: &str) -> Result<Option<String>, String> {
        credential::for_push(&self.worktree, base, &self.app_token)
    }

    pub(super) fn push(&self, candidate: &str, base: &str, token: Option<&str>) -> Result<Pushed, String> {
        let mut push = std::process::Command::new("git");
        push.arg("-C").arg(&self.worktree);
        credential::apply(&mut push, token);
        let pushed = push
            .args(["push", "-q", "--no-verify", "--no-follow-tags", &format!("--force-with-lease=refs/heads/main:{base}"), "origin", &format!("{candidate}:refs/heads/main")])
            .env("SAFE_PUSH_ACTIVE", "1")
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|error| format!("git push: {error}"))?;
        let stderr = String::from_utf8_lossy(&pushed.stderr);
        eprint!("{stderr}");
        if pushed.status.success() {
            return Ok(Pushed::Landed);
        }
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        if now.split_whitespace().next() == Some(base) {
            println!("north land: origin refused the push of {candidate} although main is still {base}");
            return Ok(Pushed::Refused(push_refusal::classify(&stderr)));
        }
        Ok(Pushed::Moved)
    }

    pub(super) fn base_moved(&self, base: &str) -> Result<bool, String> {
        let now = git(&self.worktree, &["ls-remote", "origin", "refs/heads/main"])?;
        Ok(now.split_whitespace().next() != Some(base))
    }

    pub fn round(&self) -> Result<Round, String> {
        let logs = self.queue.join("logs");
        let mut limit = usize::MAX;
        let mut red_only = false;
        let mut rebuilds = 0;
        let mut bisect_depth = 0;
        let mut searched = None;
        let mut progressed = false;
        let mut warned = false;
        after::recover(self)?;
        if let Err(error) = sheriff::patrol(self) {
            println!("north land: waiting: sheriff: {error}");
        }
        loop {
            if limit == usize::MAX {
                let reasons = self.pull_remote();
                if !warned {
                    for reason in &reasons {
                        println!("north land: waiting: {reason}");
                    }
                    warned = !reasons.is_empty();
                }
            }
            let queued = queue::collect(&self.queue).map_err(|error| error.to_string())?;
            let tickets = if red_only { red::fixes(&queued) } else { queued };
            if tickets.is_empty() {
                return Ok(if progressed { Round::Progress } else { Round::Idle });
            }
            let base = match candidate::prepare(&self.repo, &self.worktree) {
                Ok(base) => base,
                Err(error) => return Ok(Round::Waiting(format!("transient: {error}"))),
            };
            let built = candidate::build(&self.worktree, candidate::BRANCH, &base, &tickets, limit, &logs)?;
            let mut included = Vec::new();
            let mut undecided = None;
            for ((path, ticket), pick) in tickets.iter().zip(&built.picks) {
                match pick {
                    Pick::Included => included.push((path, ticket)),
                    Pick::Decided(verdict) => {
                        records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict });
                        bisect::decide(path, ticket, verdict.clone(), None)?;
                        progressed = true;
                    }
                    Pick::Undecided(reason) => undecided = Some(reason.clone()),
                    Pick::Deferred(reason) => println!("north land: {reason}"),
                }
            }
            if included.is_empty() {
                return Ok(match (progressed, undecided) {
                    (true, _) => Round::Progress,
                    (false, Some(reason)) => Round::Waiting(reason),
                    (false, None) => Round::Idle,
                });
            }
            let mut strikes = bisect::Strikes::load(&self.queue, &base)?;
            let key = bisect::Strikes::key(&included);
            if included.len() > 1 && strikes.exhausted(&key) {
                limit = bisect::half(included.len());
                bisect_depth += 1;
                println!("north land: a candidate of {} lanes kept ending without a verdict; checking its first {limit} lanes", included.len());
                continue;
            }
            let identity = self.identity()?;
            println!("north land: candidate {} carries {} lanes onto main@{base}; checking it", built.head, built.included());
            let log = logs.join(format!("{}-candidate.log", micros()));
            let mut stages = Stages::start();
            let taken = (limit == usize::MAX && !red_only && !self.spares.is_empty()).then(|| speculate::Taken::new(&tickets, &included));
            let (verdict, mut stacked) = speculate::check(self, &built.head, &base, &log, taken.as_ref());
            stages.mark("check");
            stages.attempts = rebuilds + 1;
            stages.bisect_depth = bisect_depth;
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
            let streak = if moved { 0 } else { strikes.record(&key, &verdict) };
            let verdict = bisect::escalate(verdict, streak, &log);
            strikes.save(&self.queue)?;
            let judgement = match bisect::judge(verdict, included.len(), moved) {
                Judgement::Halve(half) if searched.as_deref() != Some(base.as_str()) => {
                    stacked.clear();
                    searched = Some(base.clone());
                    match culprit::search(self, &base, &included, &logs, &mut strikes, &stages)? {
                        Found::Decided => {
                            progressed = true;
                            limit = usize::MAX;
                            bisect_depth += 1;
                            continue;
                        }
                        Found::Innocent => Judgement::Halve(half),
                        Found::Moved => Judgement::Rebuild,
                        Found::Held(reason) => Judgement::Hold(reason),
                    }
                }
                other => other,
            };
            match judgement {
                Judgement::Land => {
                    let token = match self.push_token(&base) {
                        Ok(token) => token,
                        Err(error) => return Ok(Round::Waiting(format!("transient: no push credential: {error}"))),
                    };
                    after::begin(self, &built.head, &base, &included)?;
                    match self.push(&built.head, &base, token.as_deref())? {
                        Pushed::Landed => {}
                        Pushed::Moved => {
                            after::abandon(self);
                            println!("north land: main moved from {base} before the push; rebuilding the candidate");
                            rebuilds += 1;
                            if rebuilds >= ATTEMPTS {
                                break;
                            }
                            continue;
                        }
                        Pushed::Refused(Refusal::Terminal(line)) => {
                            after::abandon(self);
                            let refusal = format!("failed origin refused the push of {}: {line}", built.head);
                            let batch_size = included.len();
                            for (path, ticket) in included {
                                records::write(self, path, ticket, &Candidate { head: &built.head, batch_size, stages: &stages }, &Decision::Refused { verdict: &refusal });
                                bisect::decide(path, ticket, refusal.clone(), None)?;
                            }
                            return Ok(Round::Progress);
                        }
                        Pushed::Refused(Refusal::Retry(line)) => {
                            after::abandon(self);
                            return Ok(Round::PushRefused(format!("origin refused the push of {} although main is still {base}: {line}", built.head)));
                        }
                    }
                    stages.mark("push");
                    after::finish(self, &built.head, &base, &stages, included.into_iter().map(|(path, ticket)| (path.clone(), ticket.clone())).collect())?;
                    speculate::land(self, stacked, &built.head)?;
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
                    bisect_depth += 1;
                }
                Judgement::Culprit(detail) => {
                    let (path, ticket) = included[0];
                    let refusal = format!("failed {detail}");
                    records::write(self, path, ticket, &Candidate { head: &built.head, batch_size: included.len(), stages: &stages }, &Decision::Refused { verdict: &refusal });
                    bisect::decide(path, ticket, refusal, Some(&log))?;
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

fn back_off(lander: &Lander, backoff: &mut Backoff, reason: &str, retire: &AtomicBool) {
    let pause = backoff.refused();
    println!("north land: waiting {}s: {reason}", pause.wait.as_secs());
    if pause.alarm && !after::send("proxy", &format!("north land: {}: {reason}; the lander keeps retrying every {} minutes", lander.repo.display(), pause.wait.as_secs() / 60)) {
        eprintln!("north land: could not alarm the proxy about {}", lander.repo.display());
    }
    let until = Instant::now() + pause.wait;
    while Instant::now() < until && !retire.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
    }
}

fn serve_repository(lander: &Lander, once: bool, retire: &AtomicBool) -> u8 {
    println!("north land: serving {} from queue {}", lander.repo.display(), lander.queue.display());
    let mut backoff = Backoff::default();
    loop {
        if retire.load(Ordering::Relaxed) {
            return 0;
        }
        let round = lander.round();
        if matches!(round, Ok(Round::Progress | Round::Idle)) {
            backoff.clear();
        }
        match round {
            Ok(Round::Progress) => continue,
            Ok(Round::PushRefused(reason)) if once => {
                println!("north land: waiting: {reason}");
                return 75;
            }
            Ok(Round::PushRefused(reason)) => {
                back_off(lander, &mut backoff, &reason, retire);
                continue;
            }
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

struct Options {
    once: bool,
    all: bool,
    repos: Vec<PathBuf>,
    checker: Vec<String>,
    depth: usize,
    app_token: Vec<String>,
}

fn parse(arguments: &[String]) -> Result<Options, ()> {
    let mut options = Options { once: false, all: false, repos: Vec::new(), checker: vec!["safe-push".to_owned()], depth: lander::DEPTH, app_token: vec![credential::MINT.to_owned()] };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.clone().next()) {
            ("--once", _) => options.once = true,
            ("--all", _) => options.all = true,
            ("--repo", Some(value)) => {
                options.repos.push(PathBuf::from(value));
                rest.next();
            }
            ("--checker", Some(value)) => {
                options.checker = vec![value.clone()];
                rest.next();
            }
            ("--app-token", Some(value)) => {
                options.app_token = vec![value.clone()];
                rest.next();
            }
            ("--depth", Some(value)) => {
                let Some(parsed) = value.parse::<usize>().ok().filter(|parsed| *parsed >= 1) else { return Err(()) };
                options.depth = parsed;
                rest.next();
            }
            _ => return Err(()),
        }
    }
    if options.repos.is_empty() && !options.all {
        options.repos.push(PathBuf::from("."));
    }
    Ok(options)
}

fn lander_for(repo: &Path, options: &Options) -> Result<Lander, String> {
    let lander = Lander::new(repo, options.checker.clone(), options.depth)?;
    Ok(Lander { app_token: options.app_token.clone(), ..lander })
}

fn exit_code(codes: impl IntoIterator<Item = u8>) -> u8 {
    codes.into_iter().fold(0, |worst, code| match (worst, code) {
        (1, _) | (_, 1) => 1,
        (75, _) | (_, 75) => 75,
        (_, other) => other,
    })
}

fn targets(options: &Options) -> (Vec<PathBuf>, Vec<String>) {
    let mut repos: Vec<PathBuf> = options.repos.iter().map(|repo| repositories::checkout(repo).unwrap_or_else(|_| repo.clone())).collect();
    let mut unserved = Vec::new();
    if options.all {
        let found = repositories::discovered(&state_home(), &repositories::local_host());
        repos.extend(found.checkouts);
        unserved = found.unserved;
    }
    repos.sort();
    repos.dedup();
    (repos, unserved)
}

fn reap_offloads() {
    let status = std::process::Command::new(after::follow::program()).args(["capacity", "offload", "reap", "--owner", lander::OFFLOAD_OWNER]).stdin(std::process::Stdio::null()).status();
    if !status.is_ok_and(|status| status.success()) {
        println!("north land: could not reap the remote offload runs of a previous lander");
    }
}

fn serve_once(options: &Options) -> u8 {
    reap_offloads();
    let (repos, unserved) = targets(options);
    for reason in unserved {
        println!("north land: not serving: {reason}");
    }
    let mut landers = Vec::new();
    for repo in &repos {
        match lander_for(repo, options) {
            Ok(lander) => landers.push(lander),
            Err(error) => {
                eprintln!("north land serve: {error}");
                return 1;
            }
        }
    }
    let retire = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let handles: Vec<_> = landers.iter().map(|lander| scope.spawn(|| serve_repository(lander, true, &retire))).collect();
        exit_code(handles.into_iter().map(|handle| handle.join().unwrap_or(1)))
    })
}

fn serve_forever(options: &Options) -> u8 {
    reap_offloads();
    let link = runtime_link();
    let started_on = runtime_target(&link);
    let retire = Arc::new(AtomicBool::new(false));
    let mut running: HashMap<PathBuf, std::thread::JoinHandle<u8>> = HashMap::new();
    let mut scanned: Option<Instant> = None;
    let mut seeded = false;
    let mut reported: HashSet<String> = HashSet::new();
    loop {
        if runtime_target(&link) != started_on {
            println!("north land: the live runtime changed; exiting so the service restarts on it");
            retire.store(true, Ordering::Relaxed);
            for (_, handle) in running.drain() {
                let _ = handle.join();
            }
            return 0;
        }
        running.retain(|_, handle| !handle.is_finished());
        if scanned.is_none_or(|at| at.elapsed() >= DISCOVERY) {
            scanned = Some(Instant::now());
            let (repos, unserved) = targets(options);
            for reason in unserved {
                if reported.insert(reason.clone()) {
                    println!("north land: not serving: {reason}");
                }
            }
            for repo in repos {
                if running.contains_key(&repo) {
                    continue;
                }
                match lander_for(&repo, options) {
                    Ok(lander) => {
                        let retire = Arc::clone(&retire);
                        running.insert(repo, std::thread::spawn(move || serve_repository(&lander, false, &retire)));
                    }
                    Err(error) if !seeded => {
                        eprintln!("north land serve: {error}");
                        return 1;
                    }
                    Err(error) => eprintln!("north land serve: {}: {error}", repo.display()),
                }
            }
            seeded = true;
        }
        std::thread::sleep(POLL);
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Ok(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if options.once { serve_once(&options) } else { serve_forever(&options) }
}
