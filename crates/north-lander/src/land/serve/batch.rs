use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{ATTEMPTS, Round, alone, micros};
use super::round::{Landing, Pushing};
use crate::land::after;
use crate::land::bisect;
use crate::land::candidate::{self, Built, Pick};
use crate::land::heartbeat::Step;
use crate::land::jobs;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;
use crate::land::watch::Watch;
use crate::land::{lander_fix, pause, steps};

pub(super) type Entry<'a> = (&'a PathBuf, &'a Ticket);

pub(super) enum Flow {
    Next,
    Stop,
    Done(Round),
}

pub(super) struct Pass {
    pub(super) red_only: bool,
    pub(super) rebuilds: usize,
    paused: Option<String>,
    pub(super) progressed: bool,
}

impl Pass {
    fn new() -> Self {
        Self { red_only: false, rebuilds: 0, paused: None, progressed: false }
    }

    pub(super) fn progress_or(&self, otherwise: Round) -> Round {
        if self.progressed { Round::Progress } else { otherwise }
    }

    pub(super) fn rebuilt(&mut self) -> Flow {
        self.rebuilds += 1;
        if self.rebuilds >= ATTEMPTS { Flow::Stop } else { Flow::Next }
    }

    pub(super) fn round<'a>(&'a self, attempt: &Attempt<'a>, strikes: BTreeMap<String, north_strategy_types::land::Streak>, slots: usize) -> steps::Round<'a> {
        steps::Round { base: attempt.base, tickets: attempt.tickets, built: attempt.built, limit: usize::MAX, red_only: self.red_only, pause: self.paused.as_deref(), strikes, spares: slots, rebuilds: self.rebuilds as u64, bisect_depth: 0 }
    }
}

pub(super) struct Attempt<'a> {
    pub(super) base: &'a str,
    pub(super) tickets: &'a [(PathBuf, Ticket)],
    pub(super) built: &'a Built,
}

pub(super) trait Batching {
    fn batch(&self, watch: &mut Watch) -> Result<Round, String>;
    fn attempt(&self, watch: &mut Watch, pass: &mut Pass, logs: &Path) -> Result<Flow, String>;
    fn gate(&self, pass: &mut Pass, queued: &[(PathBuf, Ticket)], empty: bool) -> Option<Flow>;
    fn sort_picks<'a>(&self, pass: &mut Pass, attempt: &Attempt<'a>) -> Result<(Vec<Entry<'a>>, Option<String>), String>;
    fn land_fresh(&self, watch: &mut Watch, pass: &mut Pass, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], logs: &Path) -> Result<Flow, String>;
    fn recheck(&self, pass: &mut Pass, lane: &(PathBuf, Ticket), queued: &[(PathBuf, Ticket)], logs: &Path) -> Result<(), String>;
}

pub(super) fn batch(lander: &Lander, watch: &mut Watch) -> Result<Round, String> {
    Batching::batch(lander, watch)
}

impl Batching for Lander {
    fn batch(&self, watch: &mut Watch) -> Result<Round, String> {
        let logs = self.queue.join("logs");
        let mut pass = Pass::new();
        self.round.store(u64::try_from(micros()).unwrap_or(u64::MAX), std::sync::atomic::Ordering::Relaxed);
        self.heartbeat.enter(Step::Jobs);
        watch.jobs.start(self);
        loop {
            match self.attempt(watch, &mut pass, &logs)? {
                Flow::Next => {}
                Flow::Stop => break,
                Flow::Done(round) => return Ok(round),
            }
        }
        Ok(pass.progress_or(Round::Waiting(format!("no candidate settled in {ATTEMPTS} attempts"))))
    }

    fn attempt(&self, watch: &mut Watch, pass: &mut Pass, logs: &Path) -> Result<Flow, String> {
        let claimed = jobs::claimed(self);
        let mut queued = queue::collect(&self.queue).map_err(|error| error.to_string())?;
        queued.retain(|(path, _)| !claimed.contains(path));
        let fixes;
        let tickets = if pass.red_only {
            fixes = lander_fix::repairs(&queued);
            &fixes
        } else {
            &queued
        };
        if let Some(flow) = self.gate(pass, &queued, tickets.is_empty()) {
            return Ok(flow);
        }
        self.heartbeat.enter(Step::Prepare);
        let base = match candidate::prepare(&self.repo, &self.worktree) {
            Ok(base) => base,
            Err(error) => return Ok(Flow::Done(Round::Waiting(format!("transient: {error}")))),
        };
        let (fresh, stale) = alone::split(&self.repo, &base, tickets);
        let flow = if fresh.is_empty() { Flow::Done(pass.progress_or(Round::Idle)) } else { self.land_fresh(watch, pass, &base, &fresh, &queued, logs)? };
        match (flow, stale.first()) {
            (Flow::Done(Round::Progress | Round::Idle), Some(lane)) => {
                self.recheck(pass, lane, &queued, logs)?;
                Ok(Flow::Done(pass.progress_or(Round::Waiting(format!("{} awaits its alone check", lane.1.branch)))))
            }
            (flow, _) => Ok(flow),
        }
    }

    fn recheck(&self, pass: &mut Pass, lane: &(PathBuf, Ticket), queued: &[(PathBuf, Ticket)], logs: &Path) -> Result<(), String> {
        self.heartbeat.enter(Step::Build);
        let base = match candidate::prepare(&self.repo, &self.worktree) {
            Ok(base) => base,
            Err(error) => {
                println!("north land: cannot check {} alone: transient: {error}", lane.1.branch);
                return Ok(());
            }
        };
        let alone = std::slice::from_ref(lane);
        let built = candidate::build(&self.worktree, &base, alone, queued, usize::MAX, logs)?;
        let (included, _) = self.sort_picks(pass, &Attempt { base: &base, tickets: alone, built: &built })?;
        if let [entry] = included[..] && alone::check(self, entry, &built.head, &base, logs)? {
            pass.progressed = true;
        }
        Ok(())
    }

    fn land_fresh(&self, watch: &mut Watch, pass: &mut Pass, base: &str, tickets: &[(PathBuf, Ticket)], queued: &[(PathBuf, Ticket)], logs: &Path) -> Result<Flow, String> {
        self.heartbeat.enter(Step::Build);
        let built = candidate::build(&self.worktree, base, tickets, queued, usize::MAX, logs)?;
        let attempt = Attempt { base, tickets, built: &built };
        let (included, undecided) = self.sort_picks(pass, &attempt)?;
        if included.is_empty() {
            steps::batch(&pass.round(&attempt, BTreeMap::new(), 0), &included, None);
            return Ok(Flow::Done(match (pass.progressed, undecided) {
                (true, _) => Round::Progress,
                (false, Some(reason)) => Round::Waiting(reason),
                (false, None) => Round::Idle,
            }));
        }
        let landing = Landing { head: &built.head, tips: &built.tips, worktree: &self.worktree };
        self.land_batch(watch, pass, &attempt, &included, &landing)
    }

    fn gate(&self, pass: &mut Pass, queued: &[(PathBuf, Ticket)], empty: bool) -> Option<Flow> {
        if empty {
            return Some(Flow::Done(pass.progress_or(Round::Idle)));
        }
        if !pass.red_only && let Some(reason) = pause::held(&self.queue) {
            if lander_fix::repairs(queued).is_empty() {
                return Some(Flow::Done(Round::Waiting(reason)));
            }
            println!("north land: {reason}; checking the queued red and lander fixes alone");
            pass.paused = Some(reason);
            pass.red_only = true;
            return Some(Flow::Next);
        }
        None
    }

    fn sort_picks<'a>(&self, pass: &mut Pass, attempt: &Attempt<'a>) -> Result<(Vec<Entry<'a>>, Option<String>), String> {
        let mut included = Vec::new();
        let mut undecided = None;
        for ((path, ticket), pick) in attempt.tickets.iter().zip(&attempt.built.picks) {
            match pick {
                Pick::Included => included.push((path, ticket)),
                Pick::Decided(verdict) => {
                    if verdict.starts_with("landed ") {
                        after::cleanup::report(self, path, ticket);
                    }
                    records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict });
                    bisect::decide(path, ticket, verdict.clone(), None, attempt.base)?;
                    pass.progressed = true;
                }
                Pick::Refused(refused) => {
                    records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict: &refused.verdict });
                    bisect::decide(path, ticket, refused.verdict.clone(), Some(&refused.log), attempt.base)?;
                    pass.progressed = true;
                }
                Pick::Undecided(reason) => undecided = Some(reason.clone()),
                Pick::Deferred(reason) => {
                    if let Err(error) = queue::defer(path, reason) {
                        println!("north land: could not record {}'s deferral: {error}", ticket.branch);
                    }
                    println!("north land: {reason}");
                }
            }
        }
        Ok((included, undecided))
    }
}
