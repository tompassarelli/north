use std::collections::BTreeMap;

use north_base::land_ticket::Lane;
use north_base::lifecycle::State;
use north_base::platform::attribution::{Cause, Harness};

/// What opens the message that revives a session whose registered run is over.
pub const OVER: &str = "[north: the run you waited on is over]";
const LANDING: &str = "[north: the lander has decided]";
const ADMITTED: &str = "[north: your queued run was admitted]";
pub const REVIVED: &str = "[north: dispatch revived you]";

/// Every opener a resume message starts with, which `Fleet::resumed_since_report` finds a wake by.
pub const OPENERS: [&str; 4] = [LANDING, ADMITTED, OVER, REVIVED];

/// The openers of a message that wakes a session only because something it waited on ended.
pub const WAITS: [&str; 2] = [ADMITTED, OVER];

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

pub fn message(lanes: &[Lane], ended_ms: u64) -> String {
    let decided = lanes.iter().filter(|lane| lane.queued_ms() <= ended_ms && !lane.undecided() && lane.since_ms().is_some_and(|since| since > ended_ms));
    let lines: Vec<String> = decided.map(|lane| format!("{}: {}", lane.branch, lane.cause.as_deref().unwrap_or_else(|| lane.phase.name()))).collect();
    format!("{LANDING}\n{}\nContinue from the verdict: tick the boxes a landing finishes, or fix what a refusal names.", lines.join("\n"))
}

/// A gated run that queued for capacity: when it began to queue and when the host admitted it, in Unix milliseconds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Admission {
    pub queued_ms: u64,
    pub admitted_ms: u64,
}

/// A registered wait's life: when the session registered it and when it settled, in Unix milliseconds, with the run it waited on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    pub registered_ms: u64,
    pub settled_ms: u64,
    pub run: String,
}

/// What the harness recorded for each session that it can hand back: lanes the lander holds, runs the capacity host admitted
/// after a queue, and waits that settled.
#[derive(Default)]
pub struct Held {
    pub lanes: BTreeMap<String, Vec<Lane>>,
    pub admissions: BTreeMap<String, Vec<Admission>>,
    pub settlements: BTreeMap<String, Vec<Settlement>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Waking {
    Landing,
    Capacity,
    Wait,
}

impl Waking {
    pub fn cause(self) -> Cause {
        Cause::Harness(match self {
            Self::Landing => Harness::LandingRefusal,
            Self::Capacity => Harness::Capacity,
            Self::Wait => Harness::Waits,
        })
    }

    pub fn reason(self) -> &'static str {
        match self {
            Self::Landing => "the session stopped on its queued lane and the lander decided it",
            Self::Capacity => "the session stopped while its run queued for capacity and the host admitted it",
            Self::Wait => "the session stopped on a registered wait and the wait settled",
        }
    }

    pub fn said(self) -> &'static str {
        match self {
            Self::Landing => "the lander decided its lane",
            Self::Capacity => "its queued run was admitted",
            Self::Wait => "its wait settled",
        }
    }
}

impl Held {
    /// What wakes `name`, which stopped at `ended_ms`, and the message that does: an undecided lane holds it, else the
    /// first of a decided lane, an admitted run and a settled wait, each begun before the stop and finished after it.
    pub fn waking(&self, name: &str, ended_ms: u64) -> Option<(Waking, String)> {
        let lanes = self.lanes.get(name).map_or(&[][..], Vec::as_slice);
        match landing(lanes, ended_ms) {
            Landing::Waiting => return None,
            Landing::Decided => return Some((Waking::Landing, message(lanes, ended_ms))),
            Landing::Settled => {}
        }
        let admitted = self.admissions.get(name).into_iter().flatten().any(|run| run.queued_ms <= ended_ms && run.admitted_ms > ended_ms);
        if admitted {
            return Some((Waking::Capacity, format!("{ADMITTED}\nThe run you queued for capacity was admitted after you stopped. Read its result, or run it again if it did not run, and continue to a final report.")));
        }
        let settled: Vec<&str> = self.settlements.get(name).into_iter().flatten().filter(|wait| wait.registered_ms <= ended_ms && wait.settled_ms > ended_ms).map(|wait| wait.run.as_str()).collect();
        (!settled.is_empty()).then(|| (Waking::Wait, format!("{OVER} {}: settled after you stopped. Read its output and continue to a final report.", settled.join(", "))))
    }
}
