use std::collections::BTreeMap;
use std::time::UNIX_EPOCH;

use north_base::land_ticket::Lane;
use north_base::lifecycle::State;
use north_base::platform::attribution::{Cause, Harness};

use crate::error::NorthResult;
use crate::fleet::model::{Delivery, Phase, SessionRecord, Status};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::Sender;
use crate::fleet::transcript::last_written;

/// The opener `Fleet::resumed_since_report` finds a wake by.
pub const OPENER: &str = "[north: the lander has decided]";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Landing {
    Waiting,
    Decided,
    Settled,
}

pub fn landing(lanes: &[Lane], ended_ms: u64) -> Landing {
    let mut decided = false;
    for lane in lanes.iter().filter(|lane| lane.queued_ms() <= ended_ms) {
        if lane.undecided() {
            return Landing::Waiting;
        }
        decided |= lane.since_ms().is_some_and(|since| since > ended_ms);
    }
    if decided { Landing::Decided } else { Landing::Settled }
}

pub fn ended_ms(record: &SessionRecord) -> Option<u64> {
    let ended = last_written(record)?.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(ended.as_millis()).ok()
}

pub fn message(lanes: &[Lane], ended_ms: u64) -> String {
    let decided = lanes.iter().filter(|lane| lane.queued_ms() <= ended_ms && !lane.undecided() && lane.since_ms().is_some_and(|since| since > ended_ms));
    let lines: Vec<String> = decided.map(|lane| format!("{}: {}", lane.branch, lane.cause.as_deref().unwrap_or_else(|| lane.phase.name()))).collect();
    format!("{OPENER}\n{}\nContinue from the verdict: tick the boxes a landing finishes, or fix what a refusal names.", lines.join("\n"))
}

pub trait Waker {
    fn revive(&self, name: &str) -> NorthResult<()>;
    fn attribute(&self, name: &str, cause: Cause, reason: &str);
}

pub fn resume(fleet: &Fleet, lanes: &BTreeMap<String, Vec<Lane>>, waker: &dyn Waker) -> Vec<String> {
    let mut said = Vec::new();
    for (name, held) in lanes {
        let Ok(record) = fleet.session(name) else { continue };
        let status = fleet.status(&record);
        if record.phase() == Some(Phase::Stopped) || !matches!(status, Status::Idle | Status::Parked | Status::Exited) {
            continue;
        }
        let Some(ended_ms) = ended_ms(&record) else { continue };
        if landing(held, ended_ms) != Landing::Decided || fleet.resumed_since_report(&record) {
            continue;
        }
        let woken = (if status == Status::Exited { waker.revive(name) } else { Ok(()) }).and_then(|()| fleet.send_signed(&Sender::fleet(), name, &message(held, ended_ms), Delivery::Queued));
        match woken {
            Ok(()) => {
                waker.attribute(name, Cause::Harness(Harness::LandingRefusal), "the session stopped on its queued lane and the lander decided it");
                said.push(format!("resumed {name}: the lander decided its lane"));
            }
            Err(error) => said.push(format!("not resumed {name}: {}", error.user_message())),
        }
    }
    said
}

#[cfg(test)]
#[path = "resume_tests.rs"]
mod tests;
