use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::after;
use super::bisect::{self, Strikes};
use super::candidate::{self, Pick, git};
use super::checks::{self, Started, Verdict};
use super::lander::Lander;
use super::pause;
use super::push_refusal::Refusal;
use super::queue::{self, Ticket};
use super::records::{self, Candidate, Decision};
use super::serve::{Pushed, Round};
use super::setting;
use super::slots::host::{self, CheckHost};
use super::sheriff;
use super::telemetry::Stages;

pub const TICK: Duration = Duration::from_millis(250);
const SURVEY: Duration = Duration::from_secs(2);
const HOLD: Duration = Duration::from_secs(30);
const ROOM_RETRY: Duration = Duration::from_secs(15);
const WORKTREE: &str = "lander-precheck";

type Order = (bool, u128, PathBuf);

fn order(path: &Path, ticket: &Ticket) -> Order {
    (!ticket.goes_first(), queue::queued_micros(path), path.to_owned())
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

fn slots(lander: &Lander) -> usize {
    setting::landing(&lander.repo, "refs/remotes/origin/main", "landing.slots").trim().parse().unwrap_or(0)
}

struct Lane {
    path: PathBuf,
    ticket: Ticket,
    base: String,
    head: String,
    log: PathBuf,
    stages: Stages,
}

struct Running {
    check: Option<Started>,
    branch: String,
}

impl Running {
    fn poll(&mut self) -> Option<Verdict> {
        let verdict = self.check.as_mut()?.poll()?;
        self.check = None;
        Some(verdict)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(check) = self.check.take() {
            check.kill();
            println!("north land: the check of {} stopped; its ticket was replaced or the lander stopped", self.branch);
        }
    }
}

struct Checking {
    lane: Lane,
    slot: usize,
    host: Option<CheckHost>,
    running: Running,
}

struct Held {
    reason: String,
    since: Instant,
}

#[derive(Default)]
pub struct State {
    worktrees: Vec<PathBuf>,
    checking: Vec<Checking>,
    ready: Vec<Lane>,
    recheck: Vec<Recheck>,
    held: HashMap<PathBuf, Held>,
    attempts: HashMap<PathBuf, usize>,
    strikes: Strikes,
    surveyed: Option<Instant>,
    warned: bool,
    count: usize,
    counted: Option<Instant>,
    placed: bool,
    no_room: Option<Instant>,
    draining: bool,
}

impl State {
    pub fn drain(&mut self) -> usize {
        self.draining = true;
        self.checking.len()
    }

    pub fn in_flight(&self) -> usize {
        self.checking.len()
    }

    pub fn hosts_full(&self) -> bool {
        self.no_room.is_some()
    }

    pub fn slots(&self) -> usize {
        self.count
    }

    pub fn draining(&self) -> bool {
        self.draining
    }

    pub fn waiting(&self, queued: &[(PathBuf, Ticket)]) -> usize {
        queued.iter().filter(|(path, _)| !self.tracked(path)).count()
    }

    fn count(&mut self, lander: &Lander) -> usize {
        if self.counted.is_none_or(|at| at.elapsed() >= SURVEY) {
            self.count = slots(lander);
            self.placed = host::placed(&lander.repo);
            self.counted = Some(Instant::now());
        }
        self.count
    }

    fn busy(&self) -> bool {
        !self.checking.is_empty() || !self.ready.is_empty() || !self.recheck.is_empty()
    }

    fn tracked(&self, path: &Path) -> bool {
        self.checking.iter().any(|checking| checking.lane.path == path) || self.ready.iter().any(|lane| lane.path == path) || self.recheck.iter().any(|recheck| recheck.path == path) || self.held.get(path).is_some_and(|held| held.since.elapsed() < HOLD)
    }

    fn ahead(&self, of: &Order) -> bool {
        self.checking.iter().any(|checking| order(&checking.lane.path, &checking.lane.ticket) < *of) || self.recheck.iter().any(|recheck| order(&recheck.path, &recheck.ticket) < *of)
    }

    fn forget_replaced(&mut self) {
        self.checking.retain(|checking| checking.lane.path.exists());
        self.ready.retain(|lane| lane.path.exists());
        self.recheck.retain(|recheck| recheck.path.exists());
        self.held.retain(|path, _| path.exists());
        self.attempts.retain(|path, _| path.exists());
    }

    fn free_slot(&self, count: usize) -> Option<usize> {
        (0..count.min(self.worktrees.len())).find(|slot| !self.checking.iter().any(|checking| checking.slot == *slot))
    }

    fn taken(&self) -> Vec<String> {
        self.checking.iter().filter_map(|checking| checking.host.as_ref()).map(|host| host.name().to_owned()).collect()
    }

    fn place(&mut self) -> Result<Option<CheckHost>, ()> {
        if !self.placed {
            return Ok(None);
        }
        if self.no_room.is_some_and(|at| at.elapsed() < ROOM_RETRY) {
            return Err(());
        }
        match host::choose(&self.taken()) {
            Ok(host) => {
                self.no_room = None;
                Ok(Some(host))
            }
            Err(why) => {
                println!("north land: waiting: {} precheck(s) running and no check host has room for another: {why}", self.checking.len());
                self.no_room = Some(Instant::now());
                Err(())
            }
        }
    }
}

struct Tick<'a> {
    lander: &'a Lander,
    logs: PathBuf,
    main: Option<String>,
    progressed: bool,
}

impl Tick<'_> {
    fn main(&mut self) -> Result<String, String> {
        if let Some(main) = &self.main {
            return Ok(main.clone());
        }
        let main = candidate::prepare(&self.lander.repo, &self.lander.worktree)?;
        self.main = Some(main.clone());
        Ok(main)
    }

    fn refuse(&mut self, lane: &Lane, batch_size: usize, verdict: String, log: Option<&Path>) -> Result<(), String> {
        records::write(self.lander, &lane.path, &lane.ticket, &Candidate { head: &lane.head, batch_size, stages: &lane.stages }, &Decision::Refused { verdict: &verdict });
        bisect::decide(&lane.path, &lane.ticket, verdict, log, &lane.base)?;
        self.progressed = true;
        Ok(())
    }
}

fn prepare_worktrees(lander: &Lander, state: &mut State, count: usize) -> Result<(), String> {
    while state.worktrees.len() < count {
        let worktree = candidate::lander_worktree(&lander.repo, &format!("{WORKTREE}-{}", state.worktrees.len()))?;
        candidate::ensure(&lander.repo, &worktree)?;
        state.worktrees.push(worktree);
    }
    Ok(())
}

fn start(tick: &mut Tick, state: &mut State, slot: usize, check_host: Option<CheckHost>, path: &Path, ticket: &Ticket, light: bool) -> Result<(), String> {
    let main = tick.main()?;
    let worktree = state.worktrees[slot].clone();
    let attempt = state.attempts.get(path).copied().unwrap_or(0) + 1;
    let built = candidate::build(&worktree, &main, &[(path.to_owned(), ticket.clone())], 1, &tick.logs)?;
    let name = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("lane");
    let mut lane = Lane { path: path.to_owned(), ticket: ticket.clone(), base: main.clone(), head: built.head, log: tick.logs.join(format!("{}-{name}-precheck.log", micros())), stages: Stages::start() };
    lane.stages.attempts = attempt;
    match built.picks.into_iter().next() {
        Some(Pick::Included) => {}
        Some(Pick::Decided(verdict)) => {
            lane.head.clear();
            return tick.refuse(&lane, 0, verdict, None);
        }
        Some(Pick::Undecided(reason) | Pick::Deferred(reason)) => {
            state.held.insert(lane.path, Held { reason, since: Instant::now() });
            return Ok(());
        }
        None => {
            state.held.insert(lane.path, Held { reason: format!("{} gave no pick", ticket.branch), since: Instant::now() });
            return Ok(());
        }
    }
    state.attempts.insert(lane.path.clone(), attempt);
    match checks::start(&checks::Launch { recheck: light, ..checks::Launch::of(tick.lander) }, &worktree, &main, &lane.log, true, check_host.as_ref().map(CheckHost::runner)) {
        Ok(started) => {
            let on = check_host.as_ref().map(|host| format!(" on {}", host.name())).unwrap_or_default();
            let verb = if light { "rechecks" } else { "checks" };
            println!("north land: {} {verb} alone on main@{main} in build slot {slot}{on}", ticket.branch);
            state.checking.push(Checking { running: Running { check: Some(started), branch: ticket.branch.clone() }, lane, slot, host: check_host });
            Ok(())
        }
        Err(verdict) => settle(tick, state, lane, &worktree, verdict),
    }
}

fn settle(tick: &mut Tick, state: &mut State, mut lane: Lane, worktree: &Path, verdict: Verdict) -> Result<(), String> {
    lane.stages.mark("check");
    let verdict = match verdict {
        Verdict::Green if git(worktree, &["rev-parse", "HEAD"]).ok().as_deref() != Some(lane.head.as_str()) => Verdict::Transient(format!("build slot {} moved while {} was checked", worktree.display(), lane.head)),
        other => other,
    };
    if matches!(verdict, Verdict::Green | Verdict::Refused(_)) {
        pause::resume(&tick.lander.queue);
    }
    if verdict == Verdict::Green {
        state.ready.push(lane);
        return Ok(());
    }
    let unmoved = tick.main().is_ok_and(|main| main == lane.base);
    let key = Strikes::key(&[(&lane.path, &lane.ticket)]);
    let streak = if unmoved { state.strikes.record(&key, &verdict) } else { 0 };
    let branch = lane.ticket.branch.clone();
    let reason = match bisect::escalate(verdict, streak, &lane.log) {
        Verdict::Green => unreachable!("escalate keeps a red verdict red"),
        Verdict::Refused(detail) => {
            println!("north land: {branch} refused alone; no other lane waits on it");
            let log = lane.log.clone();
            return tick.refuse(&lane, 1, format!("failed {detail}"), Some(&log));
        }
        Verdict::RedMain(detail) => format!("{branch}: main is red ({detail})"),
        Verdict::BrokenBase(detail) => format!("{branch}: {}", pause::raise(&tick.lander.queue, &detail)?),
        Verdict::Transient(detail) => format!("{branch}: transient {detail}"),
        Verdict::Deferred(detail) => format!("{branch}: deferred {detail}"),
    };
    state.held.insert(lane.path, Held { reason, since: Instant::now() });
    Ok(())
}

fn poll(tick: &mut Tick, state: &mut State) -> Result<(), String> {
    let mut index = 0;
    while index < state.checking.len() {
        let Some(verdict) = state.checking[index].running.poll() else {
            index += 1;
            continue;
        };
        let checking = state.checking.swap_remove(index);
        let worktree = state.worktrees[checking.slot].clone();
        settle(tick, state, checking.lane, &worktree, verdict)?;
    }
    Ok(())
}

struct Recheck {
    path: PathBuf,
    ticket: Ticket,
    attempts: usize,
    light: bool,
}

fn changed(worktree: &Path, from: &str, to: &str) -> Result<HashSet<String>, String> {
    Ok(git(worktree, &["diff", "--no-renames", "--name-only", from, to])?.lines().map(str::to_owned).collect())
}

fn checked_on_main(tick: &mut Tick, state: &mut State, lane: &Lane) -> Result<Option<String>, String> {
    let main = tick.main()?;
    if main == lane.base {
        return Ok(Some(lane.head.clone()));
    }
    let worktree = &tick.lander.worktree;
    let overlap: Vec<String> = changed(worktree, &lane.base, &lane.head)?.intersection(&changed(worktree, &lane.base, &main)?).cloned().collect();
    let light = overlap.is_empty();
    if light {
        println!("north land: main moved to main@{main} since {} was checked on main@{}; running the recheck lines on the tree it would land", lane.ticket.branch, lane.base);
    } else {
        println!("north land: {} changes {} that main changed since its check started; rechecking it on main@{main}", lane.ticket.branch, overlap.join(", "));
    }
    state.recheck.push(Recheck { path: lane.path.clone(), ticket: lane.ticket.clone(), attempts: lane.stages.attempts, light });
    Ok(None)
}

fn land(tick: &mut Tick, state: &mut State) -> Result<Option<Round>, String> {
    state.ready.sort_by_key(|lane| order(&lane.path, &lane.ticket));
    while let Some(first) = state.ready.first() {
        if state.ahead(&order(&first.path, &first.ticket)) {
            return Ok(None);
        }
        let mut lane = state.ready.remove(0);
        if !lane.path.exists() {
            continue;
        }
        let Some(head) = checked_on_main(tick, state, &lane)? else {
            return Ok(None);
        };
        let token = match tick.lander.push_token(&lane.base) {
            Ok(token) => token,
            Err(error) => {
                state.ready.insert(0, lane);
                return Ok(Some(Round::Waiting(format!("transient: no push credential: {error}"))));
            }
        };
        let entry = [(&lane.path, &lane.ticket)];
        after::begin(tick.lander, &head, &lane.base, &entry)?;
        match tick.lander.push(&head, &lane.base, token.as_deref())? {
            Pushed::Landed => {
                lane.stages.mark("push");
                after::finish(tick.lander, &head, &lane.base, &lane.stages, vec![(lane.path.clone(), lane.ticket.clone())])?;
                tick.main = Some(head);
                tick.progressed = true;
                state.held.clear();
            }
            Pushed::Moved => {
                after::abandon(tick.lander);
                println!("north land: main moved from {} before the push of {}; placing it again on the new main", lane.base, lane.ticket.branch);
                tick.main = None;
                state.ready.insert(0, lane);
                return Ok(None);
            }
            Pushed::Refused(Refusal::Terminal(line)) => {
                after::abandon(tick.lander);
                tick.refuse(&lane, 1, format!("failed origin refused the push of {head}: {line}"), None)?;
            }
            Pushed::Refused(Refusal::Retry(line)) => {
                after::abandon(tick.lander);
                let reason = format!("origin refused the push of {head} although main is still {}: {line}", lane.base);
                state.ready.insert(0, lane);
                return Ok(Some(Round::PushRefused(reason)));
            }
        }
    }
    Ok(None)
}

fn survey(lander: &Lander, state: &mut State) {
    if state.surveyed.is_some_and(|at| at.elapsed() < SURVEY) {
        return;
    }
    state.surveyed = Some(Instant::now());
    if let Err(error) = sheriff::patrol(lander) {
        println!("north land: waiting: sheriff: {error}");
    }
    let reasons = lander.pull_remote();
    if !state.warned {
        for reason in &reasons {
            println!("north land: waiting: {reason}");
        }
        state.warned = !reasons.is_empty();
    }
}

fn fill(tick: &mut Tick, state: &mut State, count: usize, queued: &[(PathBuf, Ticket)]) -> Result<(), String> {
    state.recheck.sort_by_key(|recheck| order(&recheck.path, &recheck.ticket));
    while let Some(slot) = state.free_slot(count) {
        if state.recheck.is_empty() && !queued.iter().any(|(path, _)| path.exists() && !state.tracked(path)) {
            break;
        }
        let Ok(check_host) = state.place() else { break };
        if let Some(recheck) = (!state.recheck.is_empty()).then(|| state.recheck.remove(0)) {
            state.attempts.insert(recheck.path.clone(), recheck.attempts);
            start(tick, state, slot, check_host, &recheck.path, &recheck.ticket, recheck.light)?;
            continue;
        }
        let Some((path, ticket)) = queued.iter().find(|(path, _)| path.exists() && !state.tracked(path)) else { break };
        state.held.remove(path);
        start(tick, state, slot, check_host, path, ticket, false)?;
    }
    Ok(())
}

pub fn round(lander: &Lander, state: &mut State) -> Result<Option<Round>, String> {
    let count = state.count(lander);
    if count == 0 && !state.busy() {
        return Ok(None);
    }
    if let Err(error) = prepare_worktrees(lander, state, count) {
        if !state.busy() {
            println!("north land: the {count} build slots cannot be prepared ({error}); checking the queue as one batch");
            return Ok(None);
        }
        println!("north land: build slot unavailable: {error}");
    }
    after::recover(lander)?;
    survey(lander, state);
    let queued = queue::collect(&lander.queue).map_err(|error| error.to_string())?;
    state.forget_replaced();
    let mut tick = Tick { lander, logs: lander.queue.join("logs"), main: None, progressed: false };
    poll(&mut tick, state)?;
    if let Some(round) = land(&mut tick, state)? {
        return Ok(Some(round));
    }
    let wanted = !state.recheck.is_empty() || queued.iter().any(|(path, _)| !state.tracked(path));
    let paused = if wanted { pause::held(&lander.queue) } else { None };
    if wanted
        && !state.draining
        && paused.is_none()
        && state.free_slot(count).is_some()
        && let Err(error) = fill(&mut tick, state, count, &queued)
    {
        if !state.busy() {
            return Ok(Some(Round::Waiting(format!("transient: {error}"))));
        }
        println!("north land: no lane started this tick: {error}");
    }
    if tick.progressed {
        return Ok(Some(Round::Progress));
    }
    if state.busy() {
        return Ok(Some(Round::Checking));
    }
    let held = paused.or_else(|| queued.iter().find_map(|(path, _)| state.held.get(path)).map(|held| held.reason.clone()));
    Ok(Some(held.map_or(Round::Idle, Round::Waiting)))
}
