use std::path::Path;

use super::refuse::refuse_unfetchable;
use super::timings::Timings;
use super::git;
use crate::land::queue::Ticket;
use crate::land::scans::outgoing;
use crate::land::slots::runner;

pub(super) fn checked_alone(root: &Path, head: String, checked_tree: &str, gate: outgoing::Gate, base: &str) -> Ticket {
    match gate {
        outgoing::Gate::Judged => {}
        outgoing::Gate::NoVerdict => println!("safe-push: the private-kind judge has no verdict yet, so the lane waits in the landing queue and the lander judges it again in its alone check before it lands."),
        outgoing::Gate::Judging => println!("safe-push: the private-kind judge is still judging this lane's text, so the lane waits in the landing queue and the lander lands it once the verdict is in."),
    }
    let unchanged = git(root, &["rev-parse", "--verify", &format!("{head}^{{tree}}")]).is_ok_and(|tree| tree == checked_tree);
    let passed = unchanged && gate == outgoing::Gate::Judged;
    let unjudged = unchanged && gate == outgoing::Gate::Judging;
    Ticket { alone_green: passed.then(|| head.clone()), alone_base: passed.then(|| base.to_owned()), unjudged_on: unjudged.then(|| base.to_owned()), head, ..Ticket::default() }
}

pub(super) fn check_lane(root: &Path, base: &str, timings: &Timings) -> Result<String, String> {
    let lane_head = git(root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    timings.stage("fails first", || runner::fails_first(root, base, &lane_head))?;
    timings.stage("crate fetch", || refuse_unfetchable(root, base, &lane_head))?;
    git(root, &["rev-parse", "--verify", &format!("{lane_head}^{{tree}}")])
}
