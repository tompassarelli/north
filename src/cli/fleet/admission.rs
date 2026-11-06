//! Holding a launch that placement refuses only because its host is saturated: with `--wait`, the
//! default for a launch a lead makes, it prints the reason and checks again until the host admits it.
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::registry::Fleet;

use super::placement::{Placed, Refusal};

const POLL: Duration = Duration::from_secs(60);
const LIMIT: Duration = Duration::from_secs(2 * 60);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Waiting {
    pub poll: Duration,
    pub limit: Duration,
}

impl Waiting {
    /// `--wait` or `--no-wait` when given, else waiting when the launch's parent is a lead (a session with no parent).
    pub fn chosen(flag: Option<bool>, fleet: &Fleet, parent: Option<&str>) -> Option<Self> {
        let lead_launch = parent.is_some_and(|parent| fleet.levels_below_lead(parent) == 0);
        flag.unwrap_or(lead_launch).then_some(Self { poll: POLL, limit: LIMIT })
    }
}

/// Queues the refusal as one fleet message to the launch's parent, which restaffs on it; a stopped parent is left alone.
pub(super) fn tell_parent(fleet: &Fleet, parent: Option<&str>, text: &str) {
    if let Some(parent) = parent {
        fleet.tell(parent, text, Delivery::Queued);
    }
}

/// Where the launch runs once placement admits it; `attempt(first)` runs placement, printing its
/// verdicts on the first call. A saturated host refuses at once without `waiting`; with it, after its limit the launch tells
/// `tell_parent` a Not done report and refuses.
pub(super) fn admitted(
    name: &str,
    waiting: Option<Waiting>,
    mut attempt: impl FnMut(bool) -> NorthResult<Result<Placed, Refusal>>,
    mut say: impl FnMut(&str),
    tell_parent: impl FnOnce(&str),
) -> NorthResult<Placed> {
    let started = Instant::now();
    let mut first = true;
    loop {
        let why = match attempt(first)? {
            Ok(placed) => return Ok(placed),
            Err(Refusal::Saturated(why)) => why,
            Err(refusal) => return Err(refusal.into()),
        };
        let Some(waiting) = waiting else { return Err(NorthError::Refused(why)) };
        if started.elapsed() >= waiting.limit {
            let reason = format!("{why}; still saturated after waiting {} min", waiting.limit.as_secs() / 60);
            tell_parent(&format!("[from {name} · launch refused] Not done: {name} was not launched: {reason}\nStop: capacity"));
            return Err(NorthError::Refused(reason));
        }
        say(&format!("fleet launch: {name} waits for admission: {why}; checking again in {} s", waiting.poll.as_secs()));
        std::thread::sleep(waiting.poll);
        first = false;
    }
}
