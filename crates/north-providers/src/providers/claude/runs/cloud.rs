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

#[cfg(test)]
mod tests {
    use super::{finished, launched};
    use crate::foundation::clock::Timestamp;
    use crate::threads::runs::CLOUD_ORIGIN;

    #[test]
    fn a_cloud_launch_row_runs_with_its_brief_and_eta_and_its_finished_row_classifies_the_last_reply() {
        let at = Timestamp::parse("2026-11-05T08:00:00Z").unwrap();
        let row = launched("session_01cloud", "Item: north#255\nCategory: tooling\nETA: 45 min", at);
        assert_eq!((row["status"].as_str(), row["brief_origin"].as_str(), row["eta_min"].as_f64(), row["started"].as_str()), (Some("running"), Some(CLOUD_ORIGIN), Some(45.0), Some("2026-11-05T08:00:00Z")));
        let done = finished("session_01cloud", at, Timestamp::parse("2026-11-05T08:41:30Z").unwrap(), "Done: landed 1234abcd on origin/main.");
        assert_eq!((done["status"].as_str(), done["actual_min"].as_i64(), done["outcome"].as_str()), (Some("finished"), Some(41), Some("done")));
    }
}
