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

    fn holds(&self, stack: &[Next], path: &PathBuf, ticket: &Ticket) -> bool {
        let wanted = lane(ticket);
        self.paths.contains(path) || self.lanes.contains(&wanted) || stack.iter().flat_map(|next| &next.tickets).any(|(held, carried)| held == path || lane(carried) == wanted)
    }
}

pub struct Next {
    worktree: PathBuf,
    base: String,
    head: String,
    identity: (String, String),
    tickets: Vec<(PathBuf, Ticket)>,
    stages: Stages,
    check: Option<Started>,
    verdict: Option<Verdict>,
}

impl Next {
    fn poll(&mut self) -> Option<&Verdict> {
        if self.verdict.is_none()
            && let Some(verdict) = self.check.as_mut().and_then(Started::poll)
        {
            self.check = None;
            self.stages.mark("check");
            self.verdict = Some(verdict);
        }
        self.verdict.as_ref()
    }

    fn red(&mut self) -> bool {
        self.poll().is_some_and(|verdict| *verdict != Verdict::Green)
    }

    fn settle(&mut self) -> Verdict {
        if let Some(check) = self.check.take() {
            self.verdict = Some(check.wait());
            self.stages.mark("check");
        }
        self.verdict.clone().unwrap_or_else(|| Verdict::Transient(format!("speculative candidate {} has no check", self.head)))
    }
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

fn fresh(lander: &Lander, taken: &Taken, stack: &[Next], seen: &mut HashSet<PathBuf>) -> Result<Vec<(PathBuf, Ticket)>, String> {
    let pending = queue::pending(&lander.queue).map_err(|error| error.to_string())?;
    let mut newest: HashMap<Lane, PathBuf> = HashMap::new();
    for (path, ticket) in &pending {
        let entry = newest.entry(lane(ticket)).or_insert_with(|| path.clone());
        if path.file_name() > entry.file_name() {
            entry.clone_from(path);
        }
    }
    let arrived: Vec<_> = pending.into_iter().filter(|(path, ticket)| !taken.holds(stack, path, ticket) && newest.get(&lane(ticket)) == Some(path)).collect();
    if arrived.iter().all(|(path, _)| seen.contains(path)) {
        return Ok(Vec::new());
    }
    seen.extend(arrived.iter().map(|(path, _)| path.clone()));
    Ok(arrived)
}

fn start(lander: &Lander, level: usize, head: &str, arrived: &[(PathBuf, Ticket)]) -> Result<Option<Next>, String> {
    let Some(worktree) = lander.spares.get(level) else { return Ok(None) };
    candidate::ensure(&lander.repo, worktree)?;
    let built = candidate::build(worktree, &candidate::speculative_branch(level), head, arrived, usize::MAX, &lander.queue.join("logs"))?;
    let tickets: Vec<_> = arrived.iter().zip(&built.picks).filter(|(_, pick)| matches!(pick, Pick::Included)).map(|(entry, _)| entry.clone()).collect();
    if tickets.is_empty() {
        return Ok(None);
    }
    let identity = identity(worktree)?;
    if identity.0 != built.head {
        return Ok(None);
    }
    let log = lander.queue.join("logs").join(format!("{}-speculative-{}.log", micros(), level + 2));
    let stages = Stages::start();
    let check = match checks::start(&lander.checker, worktree, head, &log, true) {
        Ok(check) => check,
        Err(verdict) => {
            println!("north land: speculative candidate {} not started: {verdict:?}", built.head);
            return Ok(None);
        }
    };
    println!("north land: speculative candidate {} carries {} lanes onto candidate {head}; checking it at depth {}", built.head, tickets.len(), level + 2);
    Ok(Some(Next { worktree: worktree.clone(), base: head.to_owned(), head: built.head, identity, tickets, stages, check: Some(check), verdict: None }))
}

pub fn check(lander: &Lander, head: &str, base: &str, log: &Path, taken: Option<&Taken>) -> (Verdict, Vec<Next>) {
    let mut started = match checks::start(&lander.checker, &lander.worktree, base, log, false) {
        Ok(started) => started,
        Err(verdict) => return (verdict, Vec::new()),
    };
    let mut stack: Vec<Next> = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if let Some(verdict) = started.poll() {
            return (verdict, stack);
        }
        if let Some(red) = stack.iter_mut().position(Next::red)
            && red + 1 < stack.len()
        {
            println!("north land: speculative candidate {} was not green; stopping the {} candidates stacked on it", stack[red].head, stack.len() - red - 1);
            stack.truncate(red + 1);
        }
        if let Some(taken) = taken
            && stack.len() < lander.spares.len()
            && !stack.last_mut().is_some_and(Next::red)
        {
            let parent = stack.last().map_or(head, |next| next.head.as_str()).to_owned();
            match fresh(lander, taken, &stack, &mut seen).and_then(|arrived| if arrived.is_empty() { Ok(None) } else { start(lander, stack.len(), &parent, &arrived) }) {
                Ok(Some(next)) => stack.push(next),
                Ok(None) => {}
                Err(error) => println!("north land: no speculative candidate: {error}"),
            }
        }
        std::thread::sleep(POLL);
    }
}

pub fn land(lander: &Lander, stack: Vec<Next>, landed: &str) -> Result<(), String> {
    let mut landed = landed.to_owned();
    for next in stack {
        match push(lander, next, &landed)? {
            Some(head) => landed = head,
            None => break,
        }
    }
    Ok(())
}

fn push(lander: &Lander, mut next: Next, landed: &str) -> Result<Option<String>, String> {
    if next.base != landed {
        println!("north land: speculative candidate {} was built on {}, not on the landed {landed}; discarded", next.head, next.base);
        return Ok(None);
    }
    let verdict = next.settle();
    if verdict != Verdict::Green {
        println!("north land: speculative candidate {} was not green ({verdict:?}); discarded, its lanes are rebuilt on main", next.head);
        return Ok(None);
    }
    if identity(&next.worktree)? != next.identity {
        println!("north land: the speculative worktree moved while candidate {} was checked; nothing pushed", next.head);
        return Ok(None);
    }
    if next.tickets.iter().any(|(path, _)| !path.exists()) {
        println!("north land: a lane of speculative candidate {} was decided meanwhile; discarded", next.head);
        return Ok(None);
    }
    let included: Vec<_> = next.tickets.iter().map(|(path, ticket)| (path, ticket)).collect();
    let token = match lander.push_token(&next.base) {
        Ok(token) => token,
        Err(error) => {
            println!("north land: no push credential for speculative candidate {}: {error}; discarded, its lanes are rebuilt on main", next.head);
            return Ok(None);
        }
    };
    after::begin(lander, &next.head, &next.base, &included)?;
    if !lander.push(&next.head, &next.base, token.as_deref())? {
        after::abandon(lander);
        println!("north land: main moved from {} before the speculative push; rebuilding", next.base);
        return Ok(None);
    }
    next.stages.mark("push");
    after::finish(lander, &next.head, &next.base, &next.stages, std::mem::take(&mut next.tickets))?;
    Ok(Some(next.head.clone()))
}
