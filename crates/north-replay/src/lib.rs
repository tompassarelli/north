//! `north strategy replay`: decides the last window's logged cases again with the code this binary
//! links, prints each decision kind's outcome counts beside the logged ones, and checks the declared
//! invariants over the replayed decisions (docs/rationale/strategy-kernel.md, Replay).
use std::path::Path;
use std::time::Instant;

use north_base::platform::decision_log;
use north_strategy_types::{admission, land, placement};

mod admitted;
mod bounds;
mod judged;
mod parallel;
mod placed;
mod report;

pub use bounds::Bounds;
use report::{Counts, Invariant, stamp};

const USAGE: &str = "usage: north strategy replay [--since 24h] [--invariants] [--budget]";
const DEFAULT_SINCE: f64 = 86400.0;

/// Decides a request on its logged snapshot object, answering the reason and whether the snapshot held a land check's claim.
pub type AdmissionDecider = dyn Fn(&str, &admission::Request) -> Option<(admission::Reason, bool)> + Sync;

/// The decisions replay runs again, each over a logged case.
pub struct Deciders<'a> {
    pub admission: &'a AdmissionDecider,
    pub judge: &'a (dyn Fn(&land::JudgeCase) -> land::Judgement + Sync),
    pub placement: &'a (dyn Fn(&placement::Case) -> Option<placement::Target> + Sync),
}

impl Deciders<'static> {
    /// The decisions the running binary decides with.
    pub fn linked() -> Self {
        Self {
            admission: &north_capacity::capacity::replay::decide,
            judge: &north_landing::land::judge_case::judge,
            placement: &north_dispatch::pacing::placement::case::replayed_target,
        }
    }
}

pub struct Replayed {
    pub counts: Vec<Counts>,
    pub invariants: Vec<Invariant>,
}

impl Replayed {
    /// Every reason the change is refused: an outcome count that shifted past its bound, and each breached invariant.
    pub fn refusals(&self, bounds: &Bounds, invariants: bool) -> Vec<String> {
        let shifts = self.counts.iter().flat_map(|counts| counts.shifts(bounds));
        let breaches = self.invariants.iter().filter(|_| invariants).flat_map(|invariant| invariant.breaches.iter().map(|breach| format!("{}: {breach}", invariant.name)));
        shifts.chain(breaches).collect()
    }
}

pub fn replay(root: &Path, since: f64, now: f64, deciders: &Deciders, bounds: &Bounds) -> Replayed {
    let (admitted, slot) = admitted::replay(root, since, now, deciders);
    let (placed, quota, placed_github) = placed::replay(root, since, now, deciders, bounds);
    let (judged, tail, judged_github) = judged::replay(root, since, now, deciders, bounds);
    let mut github = placed_github;
    github.merge(judged_github);
    Replayed { counts: vec![admitted, placed, judged], invariants: vec![quota, github, slot, tail] }
}

pub fn run(arguments: &[String]) -> u8 {
    run_with(&Deciders::linked(), &Bounds::from_defaults(), arguments)
}

/// What one run was asked: how far back to replay, whether to check the invariants, whether to hold the replay to its time budget.
pub struct Options {
    pub since: f64,
    pub invariants: bool,
    pub budget: bool,
}

fn options(arguments: &[String]) -> Option<Options> {
    let mut options = Options { since: DEFAULT_SINCE, invariants: false, budget: false };
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--invariants" => options.invariants = true,
            "--budget" => options.budget = true,
            "--since" => options.since = decision_log::span_seconds(rest.next()?)?,
            _ => return None,
        }
    }
    Some(options)
}

/// Runs the command with the given decisions and bounds; exits 0 when nothing is refused, 1 when the change is, 2 on bad use.
pub fn run_with(deciders: &Deciders, bounds: &Bounds, arguments: &[String]) -> u8 {
    let Some(options) = options(arguments) else { return usage() };
    let Some(root) = decision_log::root() else {
        eprintln!("north strategy replay: the decision log is off (NORTH_DECISION_LOG is empty)");
        return 2;
    };
    execute(&root, decision_log::now(), deciders, bounds, &options)
}

/// Replays `options.since` seconds of the log under `root` up to `now`, prints the report and the refusals, and returns the exit code.
pub fn execute(root: &Path, now: f64, deciders: &Deciders, bounds: &Bounds, options: &Options) -> u8 {
    let started = Instant::now();
    let replayed = replay(root, now - options.since, now, deciders, bounds);
    let seconds = started.elapsed().as_secs_f64();
    print_report(&replayed, options.invariants, now - options.since, now, seconds);
    let mut refusals = replayed.refusals(bounds, options.invariants);
    if options.budget && seconds > bounds.budget_seconds {
        refusals.push(format!("the replay took {seconds:.1} s, over the {} s of replay_budget_seconds", bounds.budget_seconds));
    }
    for refusal in &refusals {
        println!("refused: {refusal}");
    }
    u8::from(!refusals.is_empty())
}


fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn print_report(replayed: &Replayed, invariants: bool, since: f64, now: f64, seconds: f64) {
    println!("north strategy replay: {} to {}", stamp(since), stamp(now));
    for counts in &replayed.counts {
        println!("{}", counts.line());
    }
    if invariants {
        for invariant in &replayed.invariants {
            println!("{}", invariant.line());
        }
    }
    println!("replayed {} cases in {seconds:.2} s", replayed.counts.iter().map(|counts| counts.cases).sum::<usize>());
}

#[cfg(test)]
mod tests;
