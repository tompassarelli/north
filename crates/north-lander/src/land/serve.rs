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
use super::push_refusal::{self, Backoff, Pause, Refusal};
use super::hold::{self, Hold};
use super::watch::Watch;
use super::heartbeat::{self, Step};
use super::watchdog;
use super::repositories;
use north_base::foundation::exit::NO_VERDICT;
use north_runtime_follow::Follow;
use options::{Options, lander_for, parse};

mod ahead;
mod alone;
mod batch;
mod handover;
mod options;
mod outbox;
mod private_kinds;
mod round;
mod tickets;
pub mod verdicts;

pub const USAGE: &str = "usage: north land serve [--once] [--all] [--repo DIR] [--checker PROGRAM] [--app-token PROGRAM]";
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

pub enum Pushed {
    Landed,
    Moved,
    Refused(Refusal),
}

struct Serving {
    lander: Arc<Lander>,
    handle: std::thread::JoinHandle<u8>,
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

/// The serving view of one lander: the methods `north land serve` layers on it,
/// which no crate but this one may add to `Lander` itself.
pub struct Serve<'a> {
    lander: &'a Lander,
}

impl<'a> Serve<'a> {
    pub fn new(lander: &'a Lander) -> Self {
        Self { lander }
    }
}

impl std::ops::Deref for Serve<'_> {
    type Target = Lander;

    fn deref(&self) -> &Lander {
        self.lander
    }
}

impl Serve<'_> {
    pub fn push_token(&self, base: &str) -> Result<Option<String>, String> {
        credential::for_push(&self.worktree, base, &self.app_token)
    }

    pub fn push(&self, candidate: &str, base: &str, token: Option<&str>) -> Result<Pushed, String> {
        let mut push = std::process::Command::new("git");
        push.arg("-C").arg(&self.worktree);
        if let Some(token) = token {
            credential::apply(&mut push, token);
        }
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

    pub fn round(&self, watch: &mut Watch) -> Result<Round, String> {
        self.heartbeat.enter(Step::Settle);
        if watch.settle(self) || alone::settle(self, watch)? {
            return Ok(Round::Progress);
        }
        Ok(match batch::batch(self, watch)? {
            Round::Idle if watch.in_flight() > 0 => Round::Checking,
            round => round,
        })
    }
}

fn back_off(lander: &Lander, pause: Pause, reason: &str, retire: &AtomicBool) {
    println!("north land: waiting {:?}: {reason}", pause.wait);
    if pause.alarm && !after::send("proxy", &format!("north land: {}: {reason}; the lander keeps retrying every {} minutes", lander.repo.display(), pause.wait.as_secs() / 60)) {
        eprintln!("north land: could not alarm the proxy about {}", lander.repo.display());
    }
    pause.sit_out(retire);
}

fn serve_repository(lander: &Lander, once: bool, retire: &AtomicBool) -> u8 {
    let wake = &lander.wake;
    let _watchers = if once {
        for reason in lander.pull_remote() {
            println!("north land: waiting: {reason}");
        }
        None
    } else {
        Some((outbox::watch(lander, wake), tickets::watch(&lander.queue, wake), verdicts::watch(lander)))
    };
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
    let mut shown: Option<super::build_lag::Lag> = None;
    let mut map_lag = super::map_lag::Watch::default();
    let mut shown_map = None;
    loop {
        if let Some(lag) = super::build_lag::report(&lander.repo)
            && shown.as_ref().is_none_or(|shown| !shown.same_episode(&lag))
        {
            println!("north land: {}", lag.note());
            shown = Some(lag);
        }
        if let Some(note) = map_lag.line(&lander.repo).filter(|note| shown_map.as_deref() != Some(note.as_str()))
        {
            println!("north land: {note}");
            shown_map = Some(note);
        }
        if retire.load(Ordering::Relaxed) {
            return handover::hand_over(lander, &watch);
        }
        let round = Serve::new(lander).round(&mut watch);
        lander.heartbeat.enter(Step::Watchdog);
        watchdog::after_round(lander, &round);
        lander.heartbeat.enter(Step::Between);
        backoff.clear_if(matches!(round, Ok(Round::Progress | Round::Idle | Round::Checking)));
        match round {
            Ok(Round::Progress) => continue,
            Ok(Round::Checking) => {
                lander.wake.wait(POLL);
                continue;
            }
            Ok(Round::PushRefused(reason)) if once => {
                println!("north land: waiting: {reason}");
                return NO_VERDICT;
            }
            Ok(Round::PushRefused(reason)) => {
                back_off(lander, backoff.after_push_refusal(), &reason, retire);
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
                back_off(lander, backoff.after_store_error(&error), &error, retire);
                continue;
            }
        }
        lander.wake.wait(POLL);
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
    if !super::alone_flight::adoptable(&lander.queue) {
        owners.push(super::alone_flight::owner(lander));
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
    let mut runtime = Follow::current();
    let retire = Arc::new(AtomicBool::new(false));
    let mut running: HashMap<PathBuf, Serving> = HashMap::new();
    let mut scanned: Option<Instant> = None;
    let mut seeded = false;
    let mut reported: HashSet<String> = HashSet::new();
    loop {
        if let Some(next) = runtime.replaced() {
            retire.store(true, Ordering::Relaxed);
            handover::announce(&next);
            let landing: Vec<PathBuf> = running.iter().filter(|(_, serving)| handover::landing(serving.lander.heartbeat.current().0)).map(|(repo, _)| repo.clone()).collect();
            let deadline = Instant::now() + handover::BOUND;
            loop {
                let now = Instant::now();
                if now >= deadline || !landing.iter().any(|repo| running.get(repo).is_some_and(|serving| !serving.handle.is_finished())) {
                    break;
                }
                std::thread::sleep(TICK.min(deadline - now));
            }
            return 0;
        }
        running.retain(|_, serving| !serving.handle.is_finished());
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
                        let lander = Arc::new(lander);
                        let serving = Arc::clone(&lander);
                        let handle = std::thread::spawn(move || {
                            let _hold = hold;
                            serve_repository(&serving, false, &retire)
                        });
                        running.insert(repo, Serving { lander, handle });
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

