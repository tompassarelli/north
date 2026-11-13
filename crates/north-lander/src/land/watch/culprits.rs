use std::path::PathBuf;

use super::super::after;
use super::super::after::owner::{self, Reverted};
use super::super::candidate::{self, git, output};
use super::super::lander::Lander;
use super::super::post_land::Outcome;
use super::super::sheriff;
use super::{Batch, LandedLane, short};

const WORKTREE: &str = "lander-revert";

pub(super) fn blame(lander: &Lander, batch: &Batch, culprits: &[usize], detail: &str, outcome: &Outcome) {
    if culprits.is_empty() {
        inherited(lander, &batch.head, detail, outcome);
        return;
    }
    let (base, lanes) = (&batch.base, &batch.lanes);
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
            Ok(line) => println!("north land: {line}"),
            Err(error) => {
                let message = format!("north land: the post-land checks of {} failed ({detail}) and its revert could not be queued: {error}", lane.ticket.branch);
                println!("{message}");
                after::send("proxy", &message);
            }
        }
    }
}

fn inherited(lander: &Lander, head: &str, detail: &str, outcome: &Outcome) {
    match sheriff::chase(lander, head, &outcome.failing, &outcome.log) {
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

