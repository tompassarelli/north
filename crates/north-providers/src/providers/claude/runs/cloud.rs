//! The run rows of a `north cloud launch` session: a running row with its brief when it launches, and the
//! finished row `north cloud wait` writes when its turn ends, so a cloud run lands in the ledger like any other.
use serde_json::{Value, json};

use super::outcome;
use super::row::{eta, prefix};
use crate::foundation::clock::Timestamp;
use crate::threads::runs::CLOUD_ORIGIN;

pub const TIER: &str = "cloud";

pub fn launched(session: &str, brief: &str, at: Timestamp) -> Value {
    let started = at.iso();
    json!({
        "id": session, "session": session, "status": "running", "role": "worker", "tier": TIER, "provider": "claude",
        "started": started, "staffed": started, "brief": brief, "brief_text": brief, "brief_origin": CLOUD_ORIGIN, "eta_min": eta(brief),
    })
}

pub fn finished(session: &str, started: Timestamp, ended: Timestamp, report: &str) -> Value {
    json!({
        "id": session, "session": session, "status": "finished", "role": "worker", "tier": TIER, "provider": "claude",
        "started": started.iso(), "ended": ended.iso(), "actual_min": (ended.unix_seconds() - started.unix_seconds()).max(0).div_euclid(60),
        "outcome": outcome::classify(report, false), "report": prefix(report, 600),
    })
}
