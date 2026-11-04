use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::after;
use super::candidate::{self, Pick, git};
use super::checks::{self, Started, Verdict};
use super::lander::Lander;
use super::queue::{self, Ticket};
use super::telemetry::Stages;

const POLL: Duration = Duration::from_millis(100);

type Lane = (String, PathBuf, String);

fn lane(ticket: &Ticket) -> Lane {
    (ticket.host.clone(), ticket.root.clone(), ticket.branch.clone())
}

fn micros() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros())
}

pub struct Taken {
    paths: HashSet<PathBuf>,
    lanes: HashSet<Lane>,
}

impl Taken {
    pub fn new(collected: &[(PathBuf, Ticket)], included: &[(&PathBuf, &Ticket)]) -> Self {
        Self { paths: collected.iter().map(|(path, _)| path.clone()).collect(), lanes: included.iter().map(|(_, ticket)| lane(ticket)).collect() }
    }
}

pub struct Next {
    base: String,
    head: String,
    identity: (String, String),
    tickets: Vec<(PathBuf, Ticket)>,
    stages: Stages,
    check: Option<Started>,
}

impl Drop for Next {
    fn drop(&mut self) {
        if let Some(check) = self.check.take() {
            check.kill();
            println!("north land: speculative candidate {} on {} discarded; its lanes stay queued", self.head, self.base);
        }
    }
}

fn identity(worktree: &Path) -> Result<(String, String), String> {
    Ok((git(worktree, &["rev-parse", "HEAD"])?, git(worktree, &["rev-parse", "--absolute-git-dir"])?))
}

fn fresh(lander: &Lander, taken: &Taken, seen: &mut HashSet<PathBuf>) -> Result<Vec<(PathBuf, Ticket)>, String> {
    let pending = queue::pending(&lander.queue).map_err(|error| error.to_string())?;
    let mut newest: HashMap<Lane, PathBuf> = HashMap::new();
    for (path, ticket) in &pending {
        let entry = newest.entry(lane(ticket)).or_insert_with(|| path.clone());
        if path.file_name() > entry.file_name() {
            entry.clone_from(path);
        }
    }
    let arrived: Vec<_> = pending.into_iter().filter(|(path, ticket)| !taken.paths.contains(path) && !taken.lanes.contains(&lane(ticket)) && newest.get(&lane(ticket)) == Some(path)).collect();
    if arrived.iter().all(|(path, _)| seen.contains(path)) {
        return Ok(Vec::new());
    }
    seen.extend(arrived.iter().map(|(path, _)| path.clone()));
    Ok(arrived)
}

fn start(lander: &Lander, head: &str, arrived: &[(PathBuf, Ticket)]) -> Result<Option<Next>, String> {
    candidate::ensure(&lander.repo, &lander.spare)?;
    let built = candidate::build(&lander.spare, candidate::SPECULATIVE_BRANCH, head, arrived, usize::MAX, &lander.queue.join("logs"))?;
    let tickets: Vec<_> = arrived.iter().zip(&built.picks).filter(|(_, pick)| matches!(pick, Pick::Included)).map(|(entry, _)| entry.clone()).collect();
    if tickets.is_empty() {
        return Ok(None);
    }
    let identity = identity(&lander.spare)?;
    if identity.0 != built.head {
        return Ok(None);
    }
    let log = lander.queue.join("logs").join(format!("{}-speculative.log", micros()));
    let stages = Stages::start();
    let check = match checks::start(&lander.checker, &lander.spare, head, &log, true) {
        Ok(check) => check,
        Err(verdict) => {
            println!("north land: speculative candidate {} not started: {verdict:?}", built.head);
            return Ok(None);
        }
    };
    println!("north land: speculative candidate {} carries {} lanes onto candidate {head}; checking it beside that candidate", built.head, tickets.len());
    Ok(Some(Next { base: head.to_owned(), head: built.head, identity, tickets, stages, check: Some(check) }))
}

pub fn check(lander: &Lander, head: &str, base: &str, log: &Path, taken: Option<&Taken>) -> (Verdict, Option<Next>) {
    let mut started = match checks::start(&lander.checker, &lander.worktree, base, log, false) {
        Ok(started) => started,
        Err(verdict) => return (verdict, None),
    };
    let mut next = None;
    let mut seen = HashSet::new();
    loop {
        if let Some(verdict) = started.poll() {
            return (verdict, next);
        }
        if next.is_none()
            && let Some(taken) = taken
        {
            match fresh(lander, taken, &mut seen).and_then(|arrived| if arrived.is_empty() { Ok(None) } else { start(lander, head, &arrived) }) {
                Ok(speculative) => next = speculative,
                Err(error) => println!("north land: no speculative candidate: {error}"),
            }
        }
        std::thread::sleep(POLL);
    }
}

pub fn land(lander: &Lander, mut next: Next, landed: &str) -> Result<bool, String> {
    let Some(check) = next.check.take() else { return Ok(false) };
    if next.base != landed {
        check.kill();
        println!("north land: speculative candidate {} was built on {}, not on the landed {landed}; discarded", next.head, next.base);
        return Ok(false);
    }
    let verdict = check.wait();
    next.stages.mark("check");
    if verdict != Verdict::Green {
        println!("north land: speculative candidate {} was not green ({verdict:?}); discarded, its lanes are rebuilt on main", next.head);
        return Ok(false);
    }
    if identity(&lander.spare)? != next.identity {
        println!("north land: the speculative worktree moved while candidate {} was checked; nothing pushed", next.head);
        return Ok(false);
    }
    if next.tickets.iter().any(|(path, _)| !path.exists()) {
        println!("north land: a lane of speculative candidate {} was decided meanwhile; discarded", next.head);
        return Ok(false);
    }
    let included: Vec<_> = next.tickets.iter().map(|(path, ticket)| (path, ticket)).collect();
    after::begin(lander, &next.head, &next.base, &included)?;
    if !lander.push(&next.head, &next.base)? {
        after::abandon(lander);
        println!("north land: main moved from {} before the speculative push; rebuilding", next.base);
        return Ok(false);
    }
    next.stages.mark("push");
    after::finish(lander, &next.head, &next.base, &next.stages, std::mem::take(&mut next.tickets))?;
    Ok(true)
}
