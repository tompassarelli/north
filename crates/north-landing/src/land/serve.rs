use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::candidate::{self, git};
use super::lander::{Lander, state_home};
use super::after;
use super::credential;
use super::post_land;
use super::push_refusal::{self, Backoff, Refusal};
use super::hold::{self, Hold};
use super::watch::Watch;
use super::heartbeat::{self, Step};
use super::watchdog;
use super::repositories;
use north_base::foundation::exit::NO_VERDICT;
use options::{Options, lander_for, parse};

mod batch;
mod handover;
mod options;

pub const USAGE: &str = "usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--depth D] [--app-token PROGRAM]";
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
        push.args(["push", "-q", "--no-verify", "--no-follow-tags", &format!("--force-with-lease=refs/heads/main:{base}"), "origin", &format!("{candidate}:refs/heads/main")]).env("SAFE_PUSH_ACTIVE", "1");
        let pushed = match north_base::platform::process::try_run_group_traced_within(push, candidate::DEADLINE) {
            Ok(Ok(pushed)) => pushed,
            Ok(Err(stalled)) => return Err(format!("git push of {candidate} gave no answer within {} s and was killed: {stalled}", candidate::DEADLINE.as_secs())),
            Err(error) => return Err(format!("git push: {error}")),
        };
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
        self.heartbeat.enter(Step::Settle);
        if watch.settle(self) {
            return Ok(Round::Progress);
        }
        Ok(match self.batch(watch)? {
            Round::Idle if watch.in_flight() > 0 => Round::Checking,
            round => round,
        })
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
    heartbeat::watched(lander, || serve_loop(lander, once, retire))
}

fn serve_loop(lander: &Lander, once: bool, retire: &AtomicBool) -> u8 {
    println!("north land: serving {} from queue {}", lander.repo.display(), lander.queue.display());
    match super::queue::adopt(&lander.queue) {
        Ok(0) => {}
        Ok(adopted) => println!("north land: recorded the phase of {adopted} tickets that had none"),
        Err(error) => println!("north land: cannot record the phase of this queue's tickets: {error}"),
    }
    let mut backoff = Backoff::default();
    let mut watch = Watch::resume(lander);
    let mut retiring: Option<Instant> = None;
    loop {
        if retire.load(Ordering::Relaxed) {
            if let Some(code) = handover::hand_over(lander, &mut watch, &mut retiring) {
                return code;
            }
            continue;
        }
        let round = lander.round(&mut watch);
        lander.heartbeat.enter(Step::Watchdog);
        watchdog::after_round(lander, &round);
        lander.heartbeat.enter(Step::Between);
        backoff.clear_if(matches!(round, Ok(Round::Progress | Round::Idle | Round::Checking)));
        match round {
            Ok(Round::Progress) => continue,
            Ok(Round::Checking) => {
                std::thread::sleep(TICK);
                continue;
            }
            Ok(Round::PushRefused(reason)) if once => {
                println!("north land: waiting: {reason}");
                return NO_VERDICT;
            }
            Ok(Round::PushRefused(reason)) => {
                back_off(lander, &mut backoff, &reason, retire);
                continue;
            }
            Ok(Round::Idle) if once => return 0,
            Ok(Round::Waiting(reason)) if once => {
                println!("north land: waiting: {reason}");
                return NO_VERDICT;
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

enum Opened {
    Serving(Lander, Hold),
    Held(String),
}

fn open(repo: &Path, options: &Options) -> Result<Opened, String> {
    let lander = lander_for(repo, options)?;
    match hold::take(&lander.queue, &lander.repo) {
        Ok(hold) => {
            reap_offloads(&lander);
            for path in post_land::reap_scratch(&lander.repo, &lander.queue) {
                println!("north land: removed the after-landing worktree {} a previous lander of {} left", path.display(), lander.repo.display());
            }
            Ok(Opened::Serving(lander, hold))
        }
        Err(reason) if options.all => Ok(Opened::Held(reason)),
        Err(reason) => Err(reason),
    }
}

fn exit_code(codes: impl IntoIterator<Item = u8>) -> u8 {
    codes.into_iter().fold(0, |worst, code| match (worst, code) {
        (1, _) | (_, 1) => 1,
        (NO_VERDICT, _) | (_, NO_VERDICT) => NO_VERDICT,
        (_, other) => other,
    })
}

fn targets(options: &Options) -> (Vec<PathBuf>, Vec<String>) {
    let mut repos: Vec<PathBuf> = options.repos.iter().map(|repo| repositories::checkout(repo).unwrap_or_else(|_| repo.clone())).collect();
    let mut unserved = Vec::new();
    if options.all {
        let found = repositories::discovered(&state_home(), &repositories::local_host());
        repos.extend(found.checkouts.into_iter().filter(|checkout| !crate::platform::land_queue::lander_declared(checkout)));
        unserved = found.unserved;
    }
    repos.sort();
    repos.dedup();
    (repos, unserved)
}

fn reap_offloads(lander: &Lander) {
    let mut owners = vec![lander.offload_owner.clone()];
    if post_land::adoptable(&lander.queue).is_empty() {
        owners.push(post_land::after_owner(&lander.offload_owner));
    }
    for owner in owners {
        let status = std::process::Command::new(after::follow::program()).args(["capacity", "offload", "reap", "--owner", &owner]).stdin(std::process::Stdio::null()).status();
        if !status.is_ok_and(|status| status.success()) {
            println!("north land: could not reap the remote offload runs {owner} of a previous lander of {} left", lander.repo.display());
        }
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
