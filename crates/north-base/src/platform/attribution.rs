//! The side a run's outcome belongs to, and the record a boundary of the harness writes for a session it touched.
//! The lander, dispatch, capacity and the fleet write one `north cause record` row each; the
//! ledger ingest prefers those rows over the phrases of a final report (north#706, docs/rationale/attribution.md).
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use super::north_binary::north_binary;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Harness {
    Placement,
    Access,
    LandingRefusal,
    FalseRevert,
    Capacity,
    Compaction,
    Waits,
    Duplicate,
    GateRefusal,
    /// The fleet stopped the session in the middle of a turn: a stop, forget, relaunch or retirement.
    Halted,
}

impl Harness {
    pub const ALL: [Harness; 10] = [Self::Placement, Self::Access, Self::LandingRefusal, Self::FalseRevert, Self::Capacity, Self::Compaction, Self::Waits, Self::Duplicate, Self::GateRefusal, Self::Halted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Access => "access",
            Self::LandingRefusal => "landing refusal",
            Self::FalseRevert => "false revert",
            Self::Capacity => "capacity",
            Self::Compaction => "compaction",
            Self::Waits => "waits",
            Self::Duplicate => "duplicate",
            Self::GateRefusal => "gate refusal",
            Self::Halted => "halted",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// Done, or a failure the worker owns: wrong approach, a failing check of its own, `Stop: reasoning` or `Stop: leash`.
    Worker,
    Harness(Harness),
    /// Halted by Tom or stopped on his decision: neither side failed.
    Tom,
}

impl Cause {
    pub fn label(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::Harness(harness) => harness.label(),
            Self::Tom => "tom",
        }
    }

    pub fn parse(label: &str) -> Option<Self> {
        match label {
            "worker" => Some(Self::Worker),
            "tom" => Some(Self::Tom),
            other => Harness::ALL.into_iter().find(|harness| harness.label() == other).map(Self::Harness),
        }
    }
}

static OPENER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^\W*(done|not done|split|blocked)\s*[:.]").expect("opener pattern"));

/// Whether a final report starts with one of the standard openers, which a run parked after compaction lacks.
pub fn has_opener(report: &str) -> bool {
    OPENER.is_match(report)
}

/// Records, through `north cause record` in the background, that `source` decided `session`'s outcome belongs to `cause`.
pub fn record(session: &str, source: &str, cause: Cause, reason: &str) {
    if session.is_empty() {
        return;
    }
    let spawned = Command::new(north_binary()).args(["cause", "record", "--session", session, "--source", source, "--cause", cause.label(), "--reason", reason]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    match spawned {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(error) => eprintln!("north: could not record the {} cause of {session}: {error}", cause.label()),
    }
}

/// `record` for the fleet session this process runs in, named by `$NORTH_FLEET_SESSION`; nothing outside a fleet session.
pub fn record_here(source: &str, cause: Cause, reason: &str) {
    if let Some(session) = std::env::var("NORTH_FLEET_SESSION").ok().filter(|session| !session.is_empty()) {
        record(&session, source, cause, reason);
    }
}
