use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::super::after;
use super::super::after::owner::{self, Reverted};
use super::super::candidate::{self, git, output};
use super::super::checks::Verdict;
use super::super::lander::Lander;
use super::super::post_land::Outcome;
use super::super::sheriff;
use super::{Batch, LandedLane, short};

const WORKTREE: &str = "lander-revert";
const BUCK2_RED: &str = "Buck2 red";

pub(super) fn blame(lander: &Lander, batch: &Batch, culprits: &[usize], walked_back: Option<&str>, detail: &str, outcome: &Outcome) -> bool {
    if culprits.is_empty() {
        inherited(lander, &batch.head, walked_back, detail, outcome);
        return false;
    }
    let (base, lanes) = (&batch.base, &batch.lanes);
    let mut queued = false;
    for &at in culprits {
        let Some(lane) = lanes.get(at) else { continue };
        let since = if at == 0 { base } else { &lanes[at - 1].tip };
        if lane.ticket.red_fix.is_some() {
            let message = format!("north land: the post-land checks failed ({detail}) with {}, a red fix; it is not reverted again", lane.ticket.branch);
            println!("{message}");
            after::send("proxy", &message);
            continue;
        }
        match revert(lander, lane, since, detail, outcome) {
            Ok(line) => {
                queued = true;
                println!("north land: {line}");
            }
            Err(error) => {
                let message = format!("north land: the post-land checks of {} failed ({detail}) and its revert could not be queued: {error}", lane.ticket.branch);
                println!("{message}");
                after::send("proxy", &message);
            }
        }
    }
    queued
}

pub(super) fn buck2_red(lander: &Lander, head: &str, base: &str, lanes: &[LandedLane], log: &Path) -> bool {
    let [lane] = lanes else {
        let message = format!("north land: Buck2 is red at {head} across {} landed lanes, so none is reverted at once; the Cargo run after the landing decides", lanes.len());
        println!("{message}");
        after::send("proxy", &message);
        return false;
    };
    if lane.ticket.red_fix.is_some() {
        let message = format!("north land: Buck2 is red at {head} with {}, a red fix; it is not reverted again", lane.ticket.branch);
        println!("{message}");
        after::send("proxy", &message);
        return false;
    }
    let outcome = Outcome { verdict: Verdict::RedMain(BUCK2_RED.to_owned()), search: None, failing: BTreeSet::new(), superseded: false, log: log.to_owned() };
    match revert(lander, lane, base, BUCK2_RED, &outcome) {
        Ok(line) => {
            println!("north land: {line}");
            true
        }
        Err(error) => {
            let message = format!("north land: Buck2 is red at {head} and its revert of {} could not be queued: {error}", lane.ticket.branch);
            println!("{message}");
            after::send("proxy", &message);
            false
        }
    }
}

fn inherited(lander: &Lander, head: &str, walked_back: Option<&str>, detail: &str, outcome: &Outcome) {
    match sheriff::chase(lander, head, walked_back, &outcome.failing, &outcome.log) {
        Ok(line) => println!("north land: sheriff: {line}"),
        Err(error) => {
            let message = format!("north land: the post-land checks of {head} are red ({detail}) with no culprit among the batch, and the sheriff could not be asked for the earlier one: {error}");
            println!("{message}");
            after::send("proxy", &message);
        }
    }
}

fn revert(lander: &Lander, lane: &LandedLane, since: &str, detail: &str, outcome: &Outcome) -> Result<String, String> {
    let worktree: PathBuf = candidate::lander_worktree(&lander.repo, WORKTREE)?;
    let main = candidate::prepare(&lander.repo, &worktree)?;
    git(&worktree, &["reset", "-q", "--hard"])?;
    git(&worktree, &["checkout", "-q", "--detach", &main])?;
    let range = format!("{since}..{}", lane.tip);
    let commits = git(&worktree, &["rev-list", "--reverse", &range])?;
    if !output(&worktree, &["revert", "--no-commit", &range])?.status.success() {
        let _ = output(&worktree, &["revert", "--abort"]);
        git(&worktree, &["reset", "-q", "--hard", &main])?;
        return Err(format!("{range} no longer reverts cleanly onto main {}", short(&main)));
    }
    let branch = format!("revert-{}", short(&lane.tip));
    git(&worktree, &["checkout", "-q", "-B", &branch])?;
    let subject = git(&worktree, &["log", "-1", "--format=%s", &lane.tip])?;
    let trailers: Vec<String> = commits.lines().map(|commit| format!("Reverts: {commit}")).collect();
    let message = format!("Revert {subject}\n\n{} landed and its post-land checks failed on {} ({detail}), so the lander reverts it through the landing queue.\n\n{}", lane.ticket.branch, short(&lane.tip), trailers.join("\n"));
    git(&worktree, &["-c", "core.hooksPath=/dev/null", "commit", "-q", "--no-verify", "-m", &message])?;
    let head = sheriff::queue_revert(lander, &worktree, &branch)?;
    let failing = if outcome.failing.is_empty() { detail.to_owned() } else { sheriff::names(&outcome.failing.iter().cloned().collect::<Vec<_>>()) };
    let reverted = Reverted { branch: &lane.ticket.branch, commit: &lane.tip, revert: &branch, red: &lane.tip, failing: &failing, log: &outcome.log };
    println!("north land: {}", owner::tell(&owner::Cli, lane.ticket.session.as_deref(), &reverted));
    Ok(format!("revert of {} {head} queued as {branch}", lane.ticket.branch))
}

