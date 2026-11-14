//! A finished worker run's outcome, settled from the records that hold the fact rather than from its report: done when
//! its lane landed on origin/main, escalated when its session opened a gate for Tom,
//! split when its session ticked boxes of its item and left boxes it claimed open (the issue's `[x]` lines that name a sha its
//! lane landed or its Done-when check passed on), done when a run that landed nothing has a passing Done-when check of its
//! item inside its window, else not done (north#872, docs/rationale/attribution.md).
use rusqlite::{Connection, params};

use crate::foundation::clock::Timestamp;

const WEEK_S: i64 = 7 * 86_400;
const ENDED: &str = "('done', 'not done', 'escalated', 'split', 'handoff', 'unclear')";
const SLACK: &str = "'+10 minutes'";
/// The indexes that let the settle read index pages instead of every run, claim and gate row under the write lock (north#1054).
const INDEXES: [(&str, &str); 3] = [
    ("runs", "CREATE INDEX IF NOT EXISTS runs_settle ON runs(role, ended, outcome, landed_sha, landed, fleet_name, started, item, expects_landing, agent)"),
    ("events", "CREATE INDEX IF NOT EXISTS events_claims ON events(item, actor) WHERE kind = 'claim'"),
    ("boundary_causes", "CREATE INDEX IF NOT EXISTS boundary_causes_gates ON boundary_causes(session, at) WHERE source = 'gate'"),
];

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
    let window = |alias: &str| {
        format!("{alias}.task_ref = runs.item AND {alias}.exit = 0 AND {alias}.ran_ms >= {} AND {alias}.ran_ms <= {}", millis("runs.started", "'+0 seconds'"), millis("runs.ended", SLACK))
    };
    let checked = if table(db, "done_checks") { format!("coalesce(runs.expects_landing, 0) = 0 AND EXISTS (SELECT 1 FROM done_checks d WHERE {})", window("d")) } else { "0".to_owned() };
    let split = if table(db, "issue_boxes") { split(db, &window("d")) } else { "0".to_owned() };
    format!("CASE WHEN coalesce(runs.landed_sha, '') != '' OR coalesce(runs.landed, 0) = 1 THEN 'done' WHEN {gated} THEN 'escalated' WHEN {split} THEN 'split' WHEN {checked} THEN 'done' ELSE 'not done' END")
}

fn same_sha(left: &str, right: &str) -> String {
    format!("({left} LIKE {right} || '%' OR {right} LIKE {left} || '%')")
}

/// Whether the run's session ticked a box of its item, a `[x]` line naming a sha its lane landed or a passing Done-when
/// check of the item ran on (`window` selects those checks as `d`), and left open a box it claimed: a claim event of the
/// session naming a box whose line in the issue's body is still unticked.
fn split(db: &Connection, window: &str) -> String {
    let own_sha = format!("coalesce(runs.landed_sha, '') != '' AND {}", same_sha("b.sha", "runs.landed_sha"));
    let checked_sha = if table(db, "done_checks") { format!("EXISTS (SELECT 1 FROM done_checks d WHERE {window} AND {})", same_sha("b.sha", "d.head_sha")) } else { "0".to_owned() };
    let ticked = format!("EXISTS (SELECT 1 FROM issue_boxes b WHERE b.item = runs.item AND b.ticked = 1 AND b.sha != '' AND ({own_sha} OR {checked_sha}))");
    let claimed = "SELECT 1 FROM events e WHERE e.kind = 'claim' AND e.item = runs.item AND e.actor IN (runs.agent, coalesce(runs.fleet_name, '')) AND json_extract(e.data, '$.box') IS NOT NULL AND EXISTS (SELECT 1 FROM issue_boxes o WHERE o.item = e.item AND o.ticked = 0 AND (o.slug = json_extract(e.data, '$.box') OR o.slug LIKE json_extract(e.data, '$.box') || '-%' OR json_extract(e.data, '$.box') LIKE o.slug || '-%'))";
    format!("({ticked} AND EXISTS ({claimed}))")
}

/// Settles every finished worker run that ended in the last week, so a landing, a gate or a check recorded after the run's
/// own row still reaches its outcome; returns how many changed.
pub(super) fn outcomes(db: &Connection, now: Timestamp) -> rusqlite::Result<usize> {
    for (_, index) in INDEXES.iter().filter(|(name, _)| table(db, name)) {
        db.execute_batch(index)?;
    }
    let since = Timestamp::from_unix_seconds(now.unix_seconds() - WEEK_S).iso();
    let settled = outcome(db);
    db.execute(&format!("UPDATE runs SET outcome = {settled} WHERE role = 'worker' AND outcome IN {ENDED} AND ended >= ?1 AND outcome IS NOT {settled}"), params![since])
}
