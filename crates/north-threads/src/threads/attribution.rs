//! Which side a finished worker run's outcome belongs to: the worker, the harness that staffed, placed, landed or
//! gated it, or Tom. Read from the cause a boundary of the harness recorded for the run's session, never from what the
//! run's report says, so the ready bar, `north ledger flow` and tier learning share one attribution that a worker's
//! wording cannot move (north#706, north#872, docs/rationale/attribution.md).
use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub use crate::platform::attribution::{Cause, Harness};
use crate::foundation::clock::Timestamp;
use crate::platform::run_roles::worker_where;

/// What an attribution rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Done,
    /// A boundary of the harness recorded the cause for this run's session.
    Recorded,
    /// A failure no record names, left to the worker by default.
    Unclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub cause: Cause,
    pub basis: Basis,
}

/// The cause of a finished run's outcome. A Done run is the worker's; a failure takes the cause a boundary recorded for
/// its session (a gate the session opened is Tom's, a host need placement, a compacted turn compaction), else it is the
/// worker's.
pub fn judge(outcome: &str, recorded: Option<Cause>) -> Verdict {
    match recorded {
        _ if outcome == "done" => Verdict { cause: Cause::Worker, basis: Basis::Done },
        Some(cause) => Verdict { cause, basis: Basis::Recorded },
        None => Verdict { cause: Cause::Worker, basis: Basis::Unclaimed },
    }
}

pub fn attribute(outcome: &str, recorded: Option<&str>) -> Cause {
    judge(outcome, recorded.and_then(Cause::parse)).cause
}

/// The SQL expression for a run's outcome as the bars read it: a `done` run that expected a landing and has no landed sha
/// is `not done`, and a finished run whose lane landed is `done` whatever its opener said, since done is the landing on
/// origin/main and never the worker's opener (north#791, north#829).
pub fn outcome_column(db: &Connection) -> &'static str {
    let has = |column: &str| db.query_row("SELECT 1 FROM pragma_table_info('runs') WHERE name = ?1", [column], |_| Ok(())).is_ok();
    if has("expects_landing") && has("landed") && has("landed_sha") {
        "CASE WHEN runs.outcome NOT IN ('running', 'idle') AND coalesce(runs.landed_sha, '') != '' THEN 'done' WHEN runs.outcome = 'done' AND coalesce(runs.expects_landing, 0) = 1 AND coalesce(runs.landed, 0) != 1 AND runs.landed_sha IS NULL THEN 'not done' ELSE runs.outcome END"
    } else {
        "runs.outcome"
    }
}

/// The SQL expression for a run's recorded cause: the latest cause a boundary recorded for the run's fleet session, else
/// `waits` when the session registered a wait on something it did not run itself (a `farm` or `branch` row, live in
/// `fleet_waits` or settled in `fleet_wait_history`) inside the run's window, else NULL in a ledger that has neither. A `job` or
/// `capture` wait is the session's own background work: the run stays open while it holds and the session's wake judges it.
pub fn recorded_column(db: &Connection) -> String {
    let has = |sql: &str| db.query_row(sql, [], |_| Ok(())).is_ok();
    let table = |name: &str| has(&format!("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '{name}'"));
    if !has("SELECT 1 FROM pragma_table_info('runs') WHERE name = 'fleet_name'") {
        return "NULL".to_owned();
    }
    let window = "session = runs.fleet_name AND created >= coalesce(runs.started, '') AND created <= coalesce(strftime('%Y-%m-%dT%H:%M:%SZ', runs.ended, '+10 minutes'), '9')";
    let boundary = table("boundary_causes").then(|| "(SELECT cause FROM boundary_causes WHERE session = runs.fleet_name AND source != 'resume' AND at >= coalesce(runs.started, '') ORDER BY at DESC, rowid DESC LIMIT 1)".to_owned());
    let waited: Vec<String> = ["fleet_wait_history", "fleet_waits"].into_iter().filter(|name| table(name)).map(|name| format!("SELECT 'waits' AS cause FROM {name} WHERE kind IN ('farm', 'branch') AND {window}")).collect();
    let waited = (!waited.is_empty()).then(|| format!("(SELECT cause FROM ({} LIMIT 1))", waited.join(" UNION ALL ")));
    match (boundary, waited) {
        (Some(boundary), Some(waited)) => format!("coalesce({boundary}, {waited})"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => "NULL".to_owned(),
    }
}

/// The SQL expression for a run's fleet session name, empty in a ledger that records none.
pub fn fleet_column(db: &Connection) -> &'static str {
    if db.query_row("SELECT 1 FROM pragma_table_info('runs') WHERE name = 'fleet_name'", [], |_| Ok(())).is_ok() { "coalesce(runs.fleet_name, '')" } else { "''" }
}

const DAY_S: i64 = 86_400;
const VERDICT_S: i64 = 3 * 3_600;
const TOP: usize = 5;

/// One harness class's finished worker runs over a window and the API-equivalent dollars they spent.
#[derive(Debug, PartialEq)]
pub struct Loss {
    pub harness: Harness,
    pub runs: usize,
    pub usd: f64,
}

/// The finished worker runs that ended in a window: the harness classes behind them, costliest first, and the usage no record attributes.
#[derive(Debug, PartialEq)]
pub struct Reading {
    pub losses: Vec<Loss>,
    /// The runs a proxy-role session stopped, relaunched or interrupted, and their dollars, apart from the harness classes.
    pub proxy: (usize, f64),
    pub unclaimed_usd: f64,
    pub total_usd: f64,
}

impl Reading {
    pub fn unclaimed_share(&self) -> f64 {
        if self.total_usd > 0.0 { self.unclaimed_usd / self.total_usd } else { 0.0 }
    }
}

pub fn losses(db: &Connection, since: i64) -> rusqlite::Result<Reading> {
    let mut statement = db.prepare(&format!("SELECT {}, coalesce(cost_usd, 0), {} FROM runs WHERE {} AND outcome NOT IN ('running', 'idle') AND ended >= ?1", outcome_column(db), recorded_column(db), worker_where()))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, Option<String>>(2)?)))?;
    let mut by: BTreeMap<Harness, (usize, f64)> = BTreeMap::new();
    let (mut unclaimed_usd, mut total_usd, mut proxy) = (0.0, 0.0, (0, 0.0));
    for (outcome, usd, recorded) in rows.filter_map(Result::ok) {
        let verdict = judge(&outcome, recorded.as_deref().and_then(Cause::parse));
        total_usd += usd;
        if verdict.basis == Basis::Unclaimed {
            unclaimed_usd += usd;
        }
        match verdict.cause {
            Cause::Harness(harness) => {
                let held = by.entry(harness).or_default();
                held.0 += 1;
                held.1 += usd;
            }
            Cause::Proxy => {
                proxy.0 += 1;
                proxy.1 += usd;
            }
            Cause::Worker | Cause::Tom => {}
        }
    }
    let mut losses: Vec<Loss> = by.into_iter().map(|(harness, (runs, usd))| Loss { harness, runs, usd }).collect();
    losses.sort_by(|left, right| right.usd.total_cmp(&left.usd).then(right.runs.cmp(&left.runs)));
    Ok(Reading { losses, proxy, unclaimed_usd, total_usd })
}

/// The line `north ledger flow` prints: the five costliest harness classes of the last 24 h, the order the proxy staffs harness fixes in, the runs a proxy session cut short, and the share of usage no record attributes over the last 3 h and the last 24 h.
pub fn pareto_line(ledger: &Path, now: i64) -> String {
    let read = Connection::open_with_flags(ledger, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).and_then(|db| Ok((losses(&db, now - DAY_S)?, losses(&db, now - VERDICT_S)?)));
    match read {
        Ok((day, recent)) => {
            let unattributed = format!("proxy-caused {} runs ${:.2}; unattributed {:.1}% of 3 h usage, {:.1}% of 24 h", day.proxy.0, day.proxy.1, recent.unclaimed_share() * 100.0, day.unclaimed_share() * 100.0);
            if day.losses.is_empty() {
                return format!("harness losses 24 h: none; {unattributed}");
            }
            let classes = day.losses.iter().take(TOP).map(|loss| format!("{} {} runs ${:.2}", loss.harness.label(), loss.runs, loss.usd)).collect::<Vec<_>>().join(", ");
            format!("harness losses 24 h, costliest first: {classes}; {unattributed}")
        }
        Err(why) => format!("harness losses 24 h: unread: {why}"),
    }
}

