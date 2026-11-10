use std::time::Duration;

use super::{Lander, Watch};
use crate::land::heartbeat::Step;

/// The longest the lander waits for a landing already on its way to main after
/// the runtime changed. A check or a build in flight is not waited for: the
/// restarted lander reruns it on the new runtime.
pub(super) const BOUND: Duration = Duration::from_secs(30);

pub(super) fn landing(step: Step) -> bool {
    matches!(step, Step::Push | Step::Finish | Step::Speculate)
}

pub(super) fn announce(next: &str) {
    println!("north land: the live runtime changed to {next}; exiting so the service restarts on it");
}

pub(super) fn hand_over(lander: &Lander, watch: &Watch) -> u8 {
    if watch.journaled() > 0 {
        let handed = if watch.adoptable(lander) { "with every after-landing run in flight held by a runner the restarted lander adopts" } else { "with a culprit search or a runner-less after-landing step still in flight that the restarted lander reruns" };
        println!("north land: handover: the restarted lander resumes the rest of the {} journaled batch(es) as one run where they follow each other on main, {handed}", watch.journaled());
    }
    0
}
