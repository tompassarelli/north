//! A finished worker run's outcome, settled from the records that hold the fact rather than from its report: done when
//! its lane landed on origin/main, escalated when its session opened a gate for Tom, done when a run that landed nothing
//! has a passing Done-when check of its item inside its window, else not done (north#872, docs/rationale/attribution.md).
use rusqlite::{Connection, params};

use crate::foundation::clock::Timestamp;

const WEEK_S: i64 = 7 * 86_400;
const ENDED: &str = "('done', 'not done', 'escalated', 'split', 'handoff', 'unclear')";
const SLACK: &str = "'+10 minutes'";

fn table(db: &Connection, name: &str) -> bool {
    db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(())).is_ok()
}

fn millis(column: &str, shift: &str) -> String {
    format!("CAST((julianday({column}, {shift}) - 2440587.5) * 86400000 AS INTEGER)")
}

/// The outcome expression each finished worker run settles to.
fn outcome(db: &Connection) -> String {
    let until = format!("coalesce(strftime('%Y-%m-%dT%H:%M:%SZ', runs.ended, {SLACK}), '9')");
    let gated = if table(db, "boundary_causes") {
        format!("EXISTS (SELECT 1 FROM boundary_causes WHERE session = runs.fleet_name AND source = 'gate' AND at >= coalesce(runs.started, '') AND at <= {until})")
    } else {
        "0".to_owned()
    };
    let checked = if table(db, "done_checks") {
        format!(
            "coalesce(runs.expects_landing, 0) = 0 AND EXISTS (SELECT 1 FROM done_checks WHERE task_ref = runs.item AND exit = 0 AND ran_ms >= {} AND ran_ms <= {})",
            millis("runs.started", "'+0 seconds'"),
            millis("runs.ended", SLACK)
        )
    } else {
        "0".to_owned()
    };
    format!("CASE WHEN coalesce(runs.landed_sha, '') != '' OR coalesce(runs.landed, 0) = 1 THEN 'done' WHEN {gated} THEN 'escalated' WHEN {checked} THEN 'done' ELSE 'not done' END")
}

/// Settles every finished worker run that ended in the last week, so a landing, a gate or a check recorded after the run's
/// own row still reaches its outcome; returns how many changed.
pub(super) fn outcomes(db: &Connection, now: Timestamp) -> rusqlite::Result<usize> {
    let since = Timestamp::from_unix_seconds(now.unix_seconds() - WEEK_S).iso();
    let settled = outcome(db);
    db.execute(&format!("UPDATE runs SET outcome = {settled} WHERE role = 'worker' AND outcome IN {ENDED} AND ended >= ?1 AND outcome IS NOT {settled}"), params![since])
}
