//! Which side a finished worker run's outcome belongs to: the worker, the harness that staffed, placed, landed or
//! gated it, or Tom. Read from the cause a boundary of the harness recorded for the run's session, else from the
//! run's final report and its compaction count, so the ready bar, `north ledger flow` and tier learning share one
//! attribution (north#706, docs/rationale/attribution.md).
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::{Connection, OpenFlags};

pub use crate::platform::attribution::{Cause, Harness};
use crate::foundation::clock::Timestamp;
use crate::platform::attribution::has_opener;
use crate::platform::run_roles::worker_where;

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("attribution pattern")
}

static WORKER: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?im)^\W*stop:\s*(reasoning|leash)\b"));
static COMPACTED: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)\bcompacted\b|\bcontext limit\b"));
static TOM: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?im)halted by tom|\btom paused\b|^\W*stop:\s*decision\b|^\W*blocked:"));
static RULES: LazyLock<Vec<(Harness, Regex)>> = LazyLock::new(|| {
    vec![
        (Harness::Placement, pattern(r"(?im)^\W*stop:\s*host\b|\b(isn't|is not|not) on this host\b|\bthis (host|machine|session)\b[^.\n]{0,60}\b(can't|cannot|has no|lacks)\b|\b(can't|cannot|couldn't)\b[^.\n]{0,80}\b(from|on) (here|this (host|machine))\b|\bdispatch (put|placed|sent) this session\b|\b(checked|run|measured) on the laptop\b|\b(can't|cannot) reach\b|\bunreachable\b|\bno route to\b|\bnative capture\b|\bclient pair\b|\bcan't (be )?(push|pushed|land|landed|run|reach)\w*( it)? (from|on) \w+")),
        (Harness::Access, pattern(r"(?i)\b(gh|github|token|ssh|credential|broker)\b[^.\n]{0,80}\b(fails?|failed|refus\w*|expired|unreachable|could not decrypt|can't decrypt)\b")),
        (Harness::Capacity, pattern(r"(?im)^\W*stop:\s*(capacity|client|budget)\b|\bcapacity (admission )?(deferred|refused|denied)\b|\b(usage|rate) limit\b|\bout of slots\b")),
        (Harness::Waits, pattern(r"(?im)^\W*stop:\s*wait\b|\b(is |stays |still )?blocked (by|on|until)\b|\bwaits? (on|for) \S+#\d+|\bhasn't landed\b|\bis not ready\b|\bwait(s|ing)? on (a |the )?(date|clock|day|volume)\b|\b(stay open|can't be (measured|ticked|run)( yet)?) until\b|\b(can't|cannot) be measured yet\b|\breview day\b|\b\d+-day window\b|\bfull (day|24 hours)\b")),
        (Harness::FalseRevert, pattern(r"(?i)\b(lander|after-landing|red[- ]main)\b[^.\n]{0,80}\brevert|\brevert(ed)?\b[^.\n]{0,80}\b(lander|after-landing|red[- ]main)\b")),
        (Harness::Duplicate, pattern(r"(?i)\balready (on origin/main|landed|closed|done|claimed)\b|\bovertaken\b|\bduplicate (dispatch|launch|run)\b|\bsomeone else holds\b|\balready (owns|has|have) a (named )?owner\b|\balready owns\b|\balready on main\b|\bstood down\b|\b(another|other|peer) session (holds|owns|landed|is working)\b|\b(another|the same) lane\b")),
        (Harness::LandingRefusal, pattern(r"(?im)^\W*stop:\s*landing\b|\blander[^.\n]{0,40}\b(rejected|refused)\b|\bsafe-push\b[^.\n]{0,60}\brefused\b|\blanding (freeze|queue is (stuck|stalled))\b|\bqueued behind\b")),
        (Harness::GateRefusal, pattern(r"(?i)\b(gate|guard|hook|judge)\b[^.\n]{0,60}\b(refused|refuses|blocked|denied)\b")),
    ]
});

/// What an attribution rests on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Basis {
    Done,
    /// A boundary of the harness recorded the cause for this run's session.
    Recorded,
    /// A phrase rule matched the final report or the compaction count.
    Report,
    /// A failure no record names and no rule claims, left to the worker by default.
    Unclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub cause: Cause,
    pub basis: Basis,
}

/// The cause of a finished run's outcome. A Done run is the worker's; a failure takes the cause a boundary recorded for
/// its session, else the first harness class its report matches, else compaction when it says it compacted or
/// compacted and ended with no standard opener. A Split, or any other report without a standard opener, is the worker's
/// own call; the rest is the worker's by default.
pub fn judge(outcome: &str, report: &str, compactions: u32, recorded: Option<Cause>) -> Verdict {
    let by = |cause, basis| Verdict { cause, basis };
    if outcome == "done" {
        return by(Cause::Worker, Basis::Done);
    }
    if let Some(cause) = recorded {
        return by(cause, Basis::Recorded);
    }
    if WORKER.is_match(report) {
        return by(Cause::Worker, Basis::Report);
    }
    if TOM.is_match(report) {
        return by(Cause::Tom, Basis::Report);
    }
    if let Some((harness, _)) = RULES.iter().find(|(_, rule)| rule.is_match(report)) {
        return by(Cause::Harness(*harness), Basis::Report);
    }
    if (compactions > 0 && !has_opener(report)) || COMPACTED.is_match(report) {
        return by(Cause::Harness(Harness::Compaction), Basis::Report);
    }
    if outcome == "split" || !has_opener(report) {
        return by(Cause::Worker, Basis::Report);
    }
    by(Cause::Worker, Basis::Unclaimed)
}

pub fn attribute(outcome: &str, report: &str, compactions: u32, recorded: Option<&str>) -> Cause {
    judge(outcome, report, compactions, recorded.and_then(Cause::parse)).cause
}

/// The SQL expression for a run's report as attribution reads it: the 600 characters the ledger keeps, then the report's
/// last `Stop:` line, which a long report's kept head loses.
pub fn report_column(db: &Connection) -> &'static str {
    if db.query_row("SELECT 1 FROM pragma_table_info('runs') WHERE name = 'stop'", [], |_| Ok(())).is_ok() { "report || coalesce(char(10) || stop, '')" } else { "report" }
}

/// The SQL expression for a run's recorded cause: the latest cause a boundary recorded for the run's fleet session, or
/// NULL in a ledger that has not recorded any.
pub fn recorded_column(db: &Connection) -> &'static str {
    let has = |sql: &str| db.query_row(sql, [], |_| Ok(())).is_ok();
    if has("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'boundary_causes'") && has("SELECT 1 FROM pragma_table_info('runs') WHERE name = 'fleet_name'") {
        "(SELECT cause FROM boundary_causes WHERE session = runs.fleet_name AND at >= coalesce(runs.started, '') ORDER BY at DESC, rowid DESC LIMIT 1)"
    } else {
        "NULL"
    }
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

/// The finished worker runs that ended in a window: the harness classes behind them, costliest first, and the usage no record or rule attributes.
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
    let mut statement = db.prepare(&format!("SELECT outcome, {}, coalesce(compactions, 0), coalesce(cost_usd, 0), {} FROM runs WHERE {} AND outcome NOT IN ('running', 'idle') AND ended >= ?1", report_column(db), recorded_column(db), worker_where()))?;
    let rows = statement.query_map([Timestamp::from_unix_seconds(since).iso()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, u32>(2)?, row.get::<_, f64>(3)?, row.get::<_, Option<String>>(4)?)))?;
    let mut by: BTreeMap<Harness, (usize, f64)> = BTreeMap::new();
    let (mut unclaimed_usd, mut total_usd, mut proxy) = (0.0, 0.0, (0, 0.0));
    for (outcome, report, compactions, usd, recorded) in rows.filter_map(Result::ok) {
        let verdict = judge(&outcome, &report, compactions, recorded.as_deref().and_then(Cause::parse));
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

/// The line `north ledger flow` prints: the five costliest harness classes of the last 24 h, the order the proxy staffs harness fixes in, the runs a proxy session cut short, and the share of usage no record or rule attributes over the last 3 h, the window the readiness bars are judged on, and the last 24 h.
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
