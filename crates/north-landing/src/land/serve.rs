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
use super::hold::{self, Hold};
use super::watch::{LandedLane, Watch};
use super::{incidents, pause, queue, red, sheriff, speculate, watchdog};
use super::repositories;

const USAGE: &str = "usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]";
const ATTEMPTS: usize = 4;
const POLL: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(250);
const DISCOVERY: Duration = Duration::from_secs(30);

#[derive(Debug, PartialEq, Eq)]
pub enum Round {
    Idle,
    Progress,
    Checking,
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

const LANDER_INPUTS: &[&str] = &["crates/north-landing/", "crates/north-base/", "Cargo.lock", "Cargo.toml", "rust-toolchain.toml"];

struct RuntimeWatch {
    link: PathBuf,
    started_on: Option<PathBuf>,
    seen: Option<PathBuf>,
}

impl RuntimeWatch {
    fn new(link: PathBuf) -> Self {
        let started_on = runtime_target(&link);
        Self { link, seen: started_on.clone(), started_on }
    }

    fn retires<'a>(&mut self, repos: impl IntoIterator<Item = &'a Path>) -> bool {
        let target = runtime_target(&self.link);
        if target == self.seen {
            return false;
        }
        if lander_changed(self.started_on.as_deref(), target.as_deref(), repos) {
            return true;
        }
        println!("north land: the live runtime moved to {} without changing the lander; serving on", target.as_deref().unwrap_or(Path::new("?")).display());
        self.seen = target;
        false
    }
}

fn runtime_sha(target: Option<&Path>) -> Option<&str> {
    let name = target?.file_name()?.to_str()?;
    (name.len() == 40 && name.bytes().all(|byte| byte.is_ascii_hexdigit())).then_some(name)
}

fn lander_changed<'a>(from: Option<&Path>, to: Option<&Path>, repos: impl IntoIterator<Item = &'a Path>) -> bool {
    let (Some(from), Some(to)) = (runtime_sha(from), runtime_sha(to)) else {
        return true;
    };
    repos
        .into_iter()
        .find_map(|repo| git(repo, &["diff", "--no-renames", "--name-only", from, to]).ok())
        .is_none_or(|paths| paths.lines().any(|path| LANDER_INPUTS.iter().any(|input| path.starts_with(input))))
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

    pub fn round(&self, watch: &mut Watch) -> Result<Round, String> {
        if watch.settle(self) {
            return Ok(Round::Progress);
        }
        Ok(match self.batch(watch)? {
            Round::Idle if watch.in_flight() > 0 => Round::Checking,
            round => round,
        })
    }

    fn batch(&self, watch: &mut Watch) -> Result<Round, String> {
        let logs = self.queue.join("logs");
        let mut limit = usize::MAX;
        let mut red_only = false;
        let mut rebuilds = 0;
        let mut bisect_depth = 0;
        let mut searched = None;
        let mut progressed = false;
        let mut warned = false;
        self.round.store(u64::try_from(micros()).unwrap_or(u64::MAX), std::sync::atomic::Ordering::Relaxed);
        if let Some(recovered) = after::recover(self)?
            && !recovered.lanes.is_empty()
        {
            watch.follow(self, &recovered.base, &recovered.head, recovered.lanes);
        }
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
            let fixes;
            let tickets = if red_only {
                fixes = red::fixes(&queued);
                &fixes
            } else {
                &queued
            };
            if tickets.is_empty() {
                return Ok(if progressed { Round::Progress } else { Round::Idle });
            }
            if !red_only && let Some(reason) = pause::held(&self.queue) {
                if red::fixes(&queued).is_empty() {
                    return Ok(Round::Waiting(reason));
                }
                println!("north land: {reason}; checking the queued red fixes alone");
                red_only = true;
                continue;
            }
            let base = match candidate::prepare(&self.repo, &self.worktree) {
                Ok(base) => base,
                Err(error) => return Ok(Round::Waiting(format!("transient: {error}"))),
            };
            let built = candidate::build(&self.worktree, &base, tickets, &queued, limit, &logs)?;
            let mut included = Vec::new();
            let mut undecided = None;
            for ((path, ticket), pick) in tickets.iter().zip(&built.picks) {
                match pick {
                    Pick::Included => included.push((path, ticket)),
                    Pick::Decided(verdict) => {
                        if let Some(sha) = verdict.strip_prefix("landed ") {
                            after::cleanup::report(self, ticket, sha);
                        }
                        records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict });
                        bisect::decide(path, ticket, verdict.clone(), None, &base)?;
                        progressed = true;
                    }
                    Pick::Refused(refused) => {
                        records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict: &refused.verdict });
                        bisect::decide(path, ticket, refused.verdict.clone(), Some(&refused.log), &base)?;
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
            let taken = (limit == usize::MAX && !red_only && !self.spares.is_empty()).then(|| speculate::Taken::new(tickets, &included));
            let (verdict, mut stacked) = speculate::check(self, &built.head, &base, &log, taken.as_ref());
            stages.checked(&built.head);
            if matches!(verdict, Verdict::Green | Verdict::Refused(_)) {
                pause::resume(&self.queue);
            }
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
            let refusal = verdict.clone();
            let mut head = built.head.clone();
            let judgement = match bisect::judge(verdict, included.len(), moved) {
                Judgement::Halve(_) if searched.as_deref() != Some(base.as_str()) => {
                    stacked.clear();
                    searched = Some(base.clone());
                    let together = culprit::Checked { verdict: refusal, head: built.head.clone(), log: log.clone() };
                    match culprit::search(self, &base, &included, together, &logs, &mut strikes, &stages)? {
                        Found::Decided => {
                            progressed = true;
                            limit = usize::MAX;
                            bisect_depth += 1;
                            continue;
                        }
                        Found::Interaction { lanes, head: prefix } => {
                            included.truncate(lanes);
                            head = prefix;
                            Judgement::Land
                        }
                        Found::Moved => Judgement::Rebuild,
                        Found::Held(reason) => Judgement::Hold(reason),
                        Found::Paused(detail) => Judgement::Paused(detail),
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
                    after::begin(self, &head, &base, &included, &built.tips)?;
                    stages.pushing();
                    match self.push(&head, &base, token.as_deref())? {
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
                            let refusal = format!("failed origin refused the push of {head}: {line}");
                            let batch_size = included.len();
                            for (path, ticket) in included {
                                records::write(self, path, ticket, &Candidate { head: &head, batch_size, stages: &stages }, &Decision::Refused { verdict: &refusal });
                                bisect::decide(path, ticket, refusal.clone(), None, &base)?;
                            }
                            return Ok(Round::Progress);
                        }
                        Pushed::Refused(Refusal::Retry(line)) => {
                            after::abandon(self);
                            return Ok(Round::PushRefused(format!("origin refused the push of {head} although main is still {base}: {line}")));
                        }
                    }
                    stages.mark("push");
                    watch.follow(self, &base, &head, included.iter().zip(&built.tips).map(|((_, ticket), tip)| LandedLane { ticket: (*ticket).clone(), tip: tip.clone() }).collect());
                    after::finish(self, &head, &base, &stages, included.into_iter().map(|(path, ticket)| (path.clone(), ticket.clone())).collect())?;
                    speculate::land(self, watch, stacked, &head)?;
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
                    bisect::decide(path, ticket, refusal, Some(&log), &base)?;
                    return Ok(Round::Progress);
                }
                Judgement::RedMain(detail) if !red_only && red::narrows(&included) => {
                    println!("north land: main is red ({detail}); checking the red-fix lanes alone first");
                    red_only = true;
                    limit = usize::MAX;
                }
                Judgement::RedMain(detail) => return Ok(Round::Waiting(red::held(&detail, included.len()))),
                Judgement::Hold(reason) => {
                    let (path, ticket) = included[0];
                    incidents::record(&self.queue, path, ticket, incidents::held(&built.head, &base, &reason));
                    return Ok(Round::Waiting(reason));
                }
                Judgement::Paused(detail) => return Ok(Round::Waiting(pause::raise(&self.queue, &detail)?)),
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
    let mut watch = Watch::resume(lander);
    loop {
        if retire.load(Ordering::Relaxed) {
            if watch.in_flight() > 0 {
                println!("north land: the live runtime changed; exiting now, and the restarted lander reruns the {} journaled after-landing run(s)", watch.in_flight());
            }
            return 0;
        }
        let round = lander.round(&mut watch);
        watchdog::after_round(lander, watch.in_flight(), &round);
        if matches!(round, Ok(Round::Progress | Round::Idle | Round::Checking)) {
            backoff.clear();
        }
        match round {
            Ok(Round::Progress) => continue,
            Ok(Round::Checking) => {
                std::thread::sleep(TICK);
                continue;
            }
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
            Err(error) => {
                eprintln!("north land serve: {}: {error}", lander.repo.display());
                back_off(lander, &mut backoff, &error, retire);
                continue;
            }
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
    let mut options = Options { once: false, all: false, repos: Vec::new(), checker: vec!["safe-push".to_owned()], depth: lander::DEPTH, app_token: credential::mint_program() };
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

enum Opened {
    Serving(Lander, Hold),
    Held(String),
}

fn open(repo: &Path, options: &Options) -> Result<Opened, String> {
    let lander = lander_for(repo, options)?;
    match hold::take(&lander.queue, &lander.repo) {
        Ok(hold) => {
            reap_offloads(&lander);
            Ok(Opened::Serving(lander, hold))
        }
        Err(reason) if options.all => Ok(Opened::Held(reason)),
        Err(reason) => Err(reason),
    }
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

fn reap_offloads(lander: &Lander) {
    let status = std::process::Command::new(after::follow::program()).args(["capacity", "offload", "reap", "--owner", &lander.offload_owner]).stdin(std::process::Stdio::null()).status();
    if !status.is_ok_and(|status| status.success()) {
        println!("north land: could not reap the remote offload runs of a previous lander of {}", lander.repo.display());
    }
}

fn serve_once(options: &Options) -> u8 {
    let (repos, unserved) = targets(options);
    for reason in unserved {
        println!("north land: not serving: {reason}");
    }
    let mut landers = Vec::new();
    for repo in &repos {
        match open(repo, options) {
            Ok(Opened::Serving(lander, hold)) => landers.push((lander, hold)),
            Ok(Opened::Held(reason)) => println!("north land: not serving: {reason}"),
            Err(error) => {
                eprintln!("north land serve: {error}");
                return 1;
            }
        }
    }
    let retire = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let handles: Vec<_> = landers.iter().map(|(lander, _)| scope.spawn(|| serve_repository(lander, true, &retire))).collect();
        exit_code(handles.into_iter().map(|handle| handle.join().unwrap_or(1)))
    })
}

fn serve_forever(options: &Options) -> u8 {
    let mut runtime = RuntimeWatch::new(runtime_link());
    let retire = Arc::new(AtomicBool::new(false));
    let mut running: HashMap<PathBuf, std::thread::JoinHandle<u8>> = HashMap::new();
    let mut scanned: Option<Instant> = None;
    let mut seeded = false;
    let mut reported: HashSet<String> = HashSet::new();
    loop {
        if runtime.retires(running.keys().map(PathBuf::as_path)) {
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
                match open(&repo, options) {
                    Ok(Opened::Serving(lander, hold)) => {
                        let retire = Arc::clone(&retire);
                        running.insert(
                            repo,
                            std::thread::spawn(move || {
                                let _hold = hold;
                                serve_repository(&lander, false, &retire)
                            }),
                        );
                    }
                    Ok(Opened::Held(reason)) => {
                        if reported.insert(reason.clone()) {
                            println!("north land: not serving: {reason}");
                        }
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

#[cfg(test)]
#[path = "serve_tests.rs"]
mod tests;
