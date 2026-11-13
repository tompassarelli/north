use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use north_base::land_ticket::{self, Phase};
use north_base::platform::records_mirror;

use super::super::{queue, status};
use super::{Queued, git};

const STEP: Duration = Duration::from_millis(250);
const ORIGIN_STEP: Duration = Duration::from_secs(3);
const ASK_AFTER: Duration = Duration::from_secs(60);
const ANSWER_WAIT: Duration = Duration::from_secs(15);

pub(super) enum Verdict {
    Landed(String),
    Refused(String),
    Undecided,
}

/// Where a queued lane's verdict shows: its ticket in a decided area of the lander's queue, here, in this host's mirror
/// of the lander's records or, for a lane queued from another host, as the lander sent it back to this host's outbox;
/// and the destination branch on origin, which alone confirms a landing.
pub(super) struct Hand<'a> {
    pub queued: &'a Queued,
    pub state_home: &'a Path,
    pub git_dir: PathBuf,
    pub destination: &'a str,
    pub lander: &'a str,
}

struct Ask {
    at: Instant,
    told: bool,
}

struct Waiting<'a> {
    name: &'a str,
    here: PathBuf,
    areas: Vec<PathBuf>,
    started: Instant,
    quiet: Instant,
    claimed: Option<String>,
    fetched: Option<Instant>,
    unreadable: bool,
    ask: Option<Ask>,
}

impl<'a> Waiting<'a> {
    fn new(name: &'a str, here: PathBuf, areas: Vec<PathBuf>) -> Self {
        let started = Instant::now();
        Self { name, here, areas, started, quiet: started, claimed: None, fetched: None, unreadable: false, ask: None }
    }
}

impl Hand<'_> {
    pub(super) fn wait(&self, bound: Duration) -> Verdict {
        let Some(name) = self.queued.path.file_name().and_then(|name| name.to_str()) else { return Verdict::Undecided };
        let origin = &self.queued.ticket.origin;
        let here = queue::directory(self.state_home, origin);
        let mut areas: Vec<PathBuf> = [here.clone(), queue::directory(records_mirror::Records::here().home(), origin)].iter().flat_map(|queue| land_ticket::directories(queue).into_iter().skip(1)).collect();
        areas.push(here.join(queue::OUTBOX).join(queue::DECIDED));
        let mut waiting = Waiting::new(name, here, areas);
        let started = waiting.started;
        loop {
            if let Some(verdict) = self.decided(&mut waiting).or_else(|| self.confirmed(&mut waiting)) {
                return verdict;
            }
            if waiting.claimed.is_none() {
                self.keep_informed(&mut waiting);
            }
            if started.elapsed() >= bound {
                return Verdict::Undecided;
            }
            std::thread::sleep(STEP);
        }
    }

    fn decided(&self, waiting: &mut Waiting) -> Option<Verdict> {
        if waiting.claimed.is_some() {
            return None;
        }
        let ticket = &self.queued.ticket;
        let verdict = waiting.areas.iter().filter_map(|area| queue::read(&area.join(waiting.name)).ok()).filter(|decided| decided.head == ticket.head && decided.branch == ticket.branch).find_map(|decided| decided.verdict)?;
        match (Phase::decided(&verdict), verdict.split_whitespace().nth(1)) {
            (Phase::Landed, Some(commit)) => {
                println!("safe-push: the lander recorded {} as landed {commit}; confirming it on origin/{}", ticket.branch, self.destination);
                waiting.claimed = Some(commit.to_owned());
                waiting.fetched = None;
                None
            }
            _ => Some(Verdict::Refused(verdict)),
        }
    }

    fn confirmed(&self, waiting: &mut Waiting) -> Option<Verdict> {
        if !(waiting.claimed.is_some() || self.queued.outbox) || waiting.fetched.is_some_and(|at| at.elapsed() < ORIGIN_STEP) {
            return None;
        }
        waiting.fetched = Some(Instant::now());
        let remote_ref = format!("refs/remotes/origin/{}", self.destination);
        if let Err(error) = git(&self.git_dir, &["fetch", "-q", "origin", &format!("+refs/heads/{}:{remote_ref}", self.destination)]) {
            if !std::mem::replace(&mut waiting.unreadable, true) {
                println!("safe-push: cannot read origin/{}, so no landing is confirmed yet: {error}", self.destination);
            }
            return None;
        }
        waiting.unreadable = false;
        let landed = match waiting.claimed.as_deref() {
            Some(commit) => git(&self.git_dir, &["merge-base", "--is-ancestor", commit, &remote_ref]).is_ok().then(|| commit.to_owned()),
            None => self.carrier(&remote_ref),
        };
        landed.map(Verdict::Landed)
    }

    fn carrier(&self, remote_ref: &str) -> Option<String> {
        let head = &self.queued.ticket.head;
        let unlanded = git(&self.git_dir, &["cherry", remote_ref, head]).ok()?.lines().any(|line| line.starts_with('+'));
        if unlanded {
            return None;
        }
        let carried = git(&self.git_dir, &["log", "--cherry-mark", "--left-only", "--no-merges", "--format=%m%H", &format!("{remote_ref}...{head}")]).ok()?;
        carried.lines().find_map(|line| line.strip_prefix('=')).map(str::to_owned)
    }

    fn keep_informed(&self, waiting: &mut Waiting) {
        if let Some(answer) = self.answered(waiting) {
            println!("safe-push: {answer}");
            waiting.quiet = Instant::now();
        }
        self.ask_when_quiet(waiting, waiting.quiet.elapsed(), &mut io::stdout());
    }

    fn ask_when_quiet(&self, waiting: &mut Waiting, quiet: Duration, out: &mut impl Write) {
        if quiet >= ASK_AFTER {
            waiting.quiet = Instant::now();
            waiting.ask = self.ask(waiting, out);
        }
    }

    /// Asks the lander where the ticket stands: through this host's outbox, which the lander reads over its own link
    /// to this host, or straight from the queue when the lander runs here.
    fn ask(&self, waiting: &Waiting, out: &mut impl Write) -> Option<Ask> {
        let ticket = &self.queued.ticket;
        let _ = writeln!(out, "safe-push: no verdict on {} ({}) after {} s; asking the lander on {}", ticket.branch, ticket.head, waiting.started.elapsed().as_secs(), self.lander);
        if !self.queued.outbox {
            match status::report(self.state_home, &ticket.origin, waiting.name) {
                Ok(text) => { let _ = writeln!(out, "safe-push: {text}"); }
                Err(error) => { let _ = writeln!(out, "safe-push: the lander's queue here does not answer: {error}"); }
            }
            return None;
        }
        let outbox = waiting.here.join(queue::OUTBOX);
        let _ = fs::remove_file(outbox.join(queue::ANSWERS).join(waiting.name));
        match queue::write_atomic(&outbox.join(queue::ASKS).join(waiting.name), &format!("{}\n", ticket.head)) {
            Ok(()) => Some(Ask { at: Instant::now(), told: false }),
            Err(error) => {
                let _ = writeln!(out, "safe-push: cannot ask the lander, since this host's outbox {} refuses the ask: {error}", outbox.display());
                None
            }
        }
    }

    fn answered(&self, waiting: &mut Waiting) -> Option<String> {
        let answer = waiting.here.join(queue::OUTBOX).join(queue::ANSWERS).join(waiting.name);
        if let Ok(text) = fs::read_to_string(&answer) {
            let _ = fs::remove_file(&answer);
            waiting.ask = None;
            return Some(text.trim().to_owned());
        }
        let ask = waiting.ask.as_mut().filter(|ask| !ask.told && ask.at.elapsed() >= ANSWER_WAIT)?;
        ask.told = true;
        Some(format!("the lander on {} has not answered in {} s, so it is not reading this host's outbox; still waiting for the verdict here, in the records mirror and on origin/{}", self.lander, ANSWER_WAIT.as_secs(), self.destination))
    }
}

#[cfg(test)]
#[path = "verdict_tests.rs"]
mod tests;
