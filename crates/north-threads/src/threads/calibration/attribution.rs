//! Whose doing a finished worker run's outcome was: kept, the worker's (wrong approach, missed the box, ran out
//! of budget, reworked, a landing its own checks refused) or the harness's, in one cause class. Records speak
//! first (another run on the same box, the lander's incident rows and the landings that overturned a refusal,
//! a compacted run that ended with no report), then the report's `Stop:` word, then phrase rules on its opening
//! paragraph. Only worker-caused outcomes speak to a tier's fitness; the harness classes are what `north flow`
//! ranks for fixing (docs/rationale/harness-attribution.md).
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::Connection;

use crate::foundation::clock::Timestamp;

use super::read::{Raw, escalated, raw_runs};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Cause {
    Kept,
    Worker(&'static str),
    Harness(&'static str),
}

/// The harness cause classes a harness fix can remove, in the order a tie is listed; a decision that waits on Tom is
/// not the worker's but no harness lane removes it, so it is neither.
pub const HARNESS: [&str; 8] = ["placement", "landing", "false-revert", "gate-overturned", "capacity", "compaction", "wait", "duplicate"];

static STOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?mi)^\W*Stop:\s*([a-z]+)").expect("stop pattern"));

/// What a report's opening paragraph says held it up when it names no `Stop:` line, first match wins.
const PHRASES: [(&str, &[&str]); 7] = [
    ("decision", &["decision from tom", "needs a decision", "your decision", "tom's decision", "halted by tom", "answer from tom", "gate "]),
    ("placement", &["laptop", "gpu", "can't reach", "cannot reach", "no route to", "is offline", "unreachable", "native capture", "client pair"]),
    ("landing", &["lander", "landing queue", "landing run", "landing freeze", "queued behind", "not on origin/main yet", "refused the landing", "landing was refused"]),
    ("capacity", &["capacity", "usage limit", "rate limit", "free slot", "out of slots"]),
    ("duplicate", &["another session", "another lane", "same lane", "peer session"]),
    ("compaction", &["compaction", "compacted", "context limit"]),
    ("wait", &["wait until", "can't be measured yet", "cannot be measured yet", "longer wait", "measurement over time", "review day", "-day window"]),
];

/// What the records hold about one run, beside its report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Records {
    /// Another worker session ran the same box while this one did, having started first.
    pub duplicate: bool,
    /// The run was compacted at least once.
    pub compacted: bool,
    /// The lander's incident rows on the run's lanes while it ran, each with whether a later landing of the
    /// same tip overturned it.
    pub incidents: Vec<Incident>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Incident {
    pub class: String,
    pub detail: String,
    pub overturned: bool,
}

impl Incident {
    fn cause(&self) -> Option<Cause> {
        if let Some(named) = HARNESS.iter().find(|name| **name == self.class) {
            return Some(Cause::Harness(named));
        }
        let revert = self.detail.to_lowercase().contains("revert");
        match self.class.as_str() {
            "lander-refused" | "lander-withdrawn" if self.overturned => Some(Cause::Harness(if revert { "false-revert" } else { "gate-overturned" })),
            "lander-refused" => Some(Cause::Worker("refused")),
            "lander-hang" | "lander-transient" | "lander-deferred" | "lander-conflict" | "lander-held" => Some(Cause::Harness("landing")),
            _ => None,
        }
    }
}

/// The cause of a run's outcome, or None when it says nothing about the worker or the harness: a split,
/// which re-scopes the box, or an unclear end with no reason a rule or record can read.
pub fn cause(outcome: &str, succeeded: bool, report: &str, records: &Records) -> Option<Cause> {
    if outcome == "split" {
        return None;
    }
    if succeeded {
        return Some(Cause::Kept);
    }
    if records.duplicate {
        return Some(Cause::Harness("duplicate"));
    }
    let recorded: Vec<Cause> = records.incidents.iter().filter_map(Incident::cause).collect();
    if let Some(harness) = recorded.iter().find(|cause| matches!(cause, Cause::Harness(_))).or(recorded.first()) {
        return Some(*harness);
    }
    if let Some(stopped) = STOP.captures(report).and_then(|found| stopped(&found[1].to_lowercase())) {
        return Some(stopped);
    }
    let opening = report.trim_start().split("\n\n").next().unwrap_or_default().to_lowercase();
    if let Some((kind, _)) = PHRASES.iter().find(|(_, phrases)| phrases.iter().any(|phrase| opening.contains(phrase))) {
        return Some(Cause::Harness(kind));
    }
    match outcome {
        "done" => Some(Cause::Worker("reworked")),
        "unclear" | "handoff" if records.compacted && report.trim().is_empty() => Some(Cause::Harness("compaction")),
        "unclear" | "handoff" => None,
        _ => Some(Cause::Worker("missed")),
    }
}

/// The cause a report's own `Stop:` word names.
fn stopped(word: &str) -> Option<Cause> {
    match word {
        "reasoning" => Some(Cause::Worker("reasoning")),
        "leash" => Some(Cause::Worker("leash")),
        "landing" => Some(Cause::Harness("landing")),
        "capacity" => Some(Cause::Harness("capacity")),
        "client" | "host" => Some(Cause::Harness("placement")),
        "wait" => Some(Cause::Harness("wait")),
        "decision" => Some(Cause::Harness("decision")),
        _ => None,
    }
}

/// One finished worker run with its attributed cause.
#[derive(Clone, Debug)]
pub struct Attributed {
    pub raw: Raw,
    pub ended: f64,
    pub first_try: bool,
    pub cause: Option<Cause>,
}

impl Attributed {
    pub fn usd(&self) -> f64 {
        self.raw.api_usd.or(self.raw.cost_usd).unwrap_or(0.0)
    }

    pub fn harness(&self) -> Option<&'static str> {
        match self.cause {
            Some(Cause::Harness(class)) if HARNESS.contains(&class) => Some(class),
            _ => None,
        }
    }
}

fn seconds(stamp: &str) -> f64 {
    Timestamp::parse_loose(stamp).map_or(f64::NAN, |at| at.unix_seconds() as f64)
}

struct Extra {
    session: String,
    report: String,
    compactions: i64,
}

struct Row {
    class: String,
    repo: String,
    subject: String,
    detected: f64,
    detail: String,
    head: String,
}

fn incidents(db: &Connection) -> rusqlite::Result<Vec<Row>> {
    let present: bool = db.query_row("SELECT count(*) = 1 FROM sqlite_master WHERE type = 'table' AND name = 'incidents'", [], |row| row.get(0))?;
    if !present {
        return Ok(Vec::new());
    }
    let mut statement = db.prepare("SELECT key, class, repo, subject, detected_at, detail FROM incidents")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get::<_, String>(4)?, row.get(5)?)))?;
    rows.map(|row| {
        row.map(|(key, class, repo, subject, detected, detail): (String, String, String, String, String, String)| Row {
            head: key.rsplit_once('@').map(|(_, head)| head.to_owned()).unwrap_or_default(),
            detected: detected.parse::<f64>().unwrap_or_else(|_| seconds(&detected)),
            class,
            repo,
            subject,
            detail,
        })
    })
    .collect()
}

/// Each tip a landing took to main, with when.
fn landed_tips(db: &Connection) -> rusqlite::Result<BTreeMap<String, f64>> {
    let present: bool = db.query_row("SELECT count(*) = 1 FROM sqlite_master WHERE type = 'table' AND name = 'landings'", [], |row| row.get(0))?;
    if !present {
        return Ok(BTreeMap::new());
    }
    let mut statement = db.prepare("SELECT tip, coalesce(landed_at, '') FROM landings WHERE verdict = 'landed'")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    rows.map(|row| row.map(|(tip, at)| (tip, seconds(&at)))).collect()
}

/// The repository and number an item names, as `north` and `706` from `tompassarelli/north#706`.
fn box_of(item: &str) -> Option<(&str, &str)> {
    let (repo, number) = item.split_whitespace().next()?.split_once('#')?;
    let number = number.trim_end_matches(|ch: char| !ch.is_ascii_digit());
    (!number.is_empty() && number.chars().all(|ch| ch.is_ascii_digit())).then(|| (repo.rsplit('/').next().unwrap_or(repo), number))
}

/// Whether a lane's branch names the box's number as a whole number, as `north-706-x` or `n706-x` do.
fn names(branch: &str, number: &str) -> bool {
    let digits: Vec<&str> = branch.split(|ch: char| !ch.is_ascii_digit()).collect();
    digits.contains(&number)
}

/// Every worker run that ended in `[since, until)` with its cause, read from the runs, incidents and landings tables.
pub fn attribute(db: &Connection, since: f64, until: f64) -> rusqlite::Result<Vec<Attributed>> {
    let followed = escalated(db)?;
    let raws = raw_runs(db, "outcome NOT IN ('running','idle') AND ended IS NOT NULL", &[])?;
    let mut statement = db.prepare("SELECT agent, coalesce(session, ''), report, coalesce(compactions, 0) FROM runs")?;
    let extra: BTreeMap<String, Extra> = statement
        .query_map([], |row| Ok((row.get::<_, String>(0)?, Extra { session: row.get(1)?, report: row.get(2)?, compactions: row.get(3)? })))?
        .collect::<rusqlite::Result<_>>()?;
    let rows = incidents(db)?;
    let tips = landed_tips(db)?;
    let span = |raw: &Raw| {
        let ended = seconds(&raw.ended);
        (raw.started.as_deref().map_or(ended - raw.minutes.unwrap_or(0.0) * 60.0, seconds), ended)
    };
    let mut by_item: BTreeMap<&str, Vec<(&Raw, f64, f64)>> = BTreeMap::new();
    for raw in &raws {
        if let Some(item) = raw.item.as_deref().filter(|item| !item.is_empty()) {
            let (started, ended) = span(raw);
            by_item.entry(item).or_default().push((raw, started, ended));
        }
    }
    let empty = Extra { session: String::new(), report: String::new(), compactions: 0 };
    let session = |agent: &str| extra.get(agent).map_or("", |found| found.session.as_str());
    Ok(raws
        .iter()
        .filter_map(|raw| {
            let (started, ended) = span(raw);
            (ended >= since && ended < until && started.is_finite()).then_some((raw, started, ended))
        })
        .map(|(raw, started, ended)| {
            let found = extra.get(&raw.agent).unwrap_or(&empty);
            let item = raw.item.as_deref().unwrap_or_default();
            let duplicate = by_item.get(item).is_some_and(|peers| {
                peers.iter().any(|(peer, from, to)| peer.agent != raw.agent && *from < started && *to > started && (session(&peer.agent) != found.session || found.session.is_empty()))
            });
            let incidents = box_of(item)
                .map(|(repo, number)| {
                    rows.iter()
                        .filter(|row| row.detected >= started && row.detected <= ended && names(&row.subject, number) && row.repo.rsplit('/').next().is_none_or(|name| name.is_empty() || name == repo))
                        .map(|row| Incident { class: row.class.clone(), detail: row.detail.clone(), overturned: !row.head.is_empty() && tips.get(&row.head).is_some_and(|at| !at.is_finite() || *at >= row.detected) })
                        .collect()
                })
                .unwrap_or_default();
            let records = Records { duplicate, compacted: found.compactions > 0, incidents };
            let succeeded = raw.succeeded(followed.contains(&raw.agent));
            Attributed { cause: cause(&raw.outcome, succeeded, &found.report, &records), first_try: raw.follows.is_none(), ended, raw: raw.clone() }
        })
        .collect())
}

/// The harness classes by runs lost, with their API-equivalent dollars, most first.
pub fn pareto(runs: &[Attributed]) -> Vec<(&'static str, usize, f64)> {
    let mut classes: BTreeMap<&'static str, (usize, f64)> = BTreeMap::new();
    for run in runs {
        if let Some(class) = run.harness() {
            let entry = classes.entry(class).or_default();
            entry.0 += 1;
            entry.1 += run.usd();
        }
    }
    let order = |class: &str| HARNESS.iter().position(|name| *name == class).unwrap_or(HARNESS.len());
    let mut ranked: Vec<(&'static str, usize, f64)> = classes.into_iter().map(|(class, (count, usd))| (class, count, usd)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)).then(order(a.0).cmp(&order(b.0))));
    ranked
}

/// The share of attributed first tries the harness lost, and how many first tries were attributed.
pub fn harness_share<'a>(runs: impl Iterator<Item = &'a Attributed>) -> Option<(f64, usize)> {
    let judged: Vec<&Attributed> = runs.filter(|run| run.first_try && run.cause.is_some() && run.cause != Some(Cause::Harness("decision"))).collect();
    let lost = judged.iter().filter(|run| run.harness().is_some()).count();
    (!judged.is_empty()).then(|| (lost as f64 / judged.len() as f64, judged.len()))
}

/// The agents of the runs whose outcome was the worker's, for the tier learning that reads only those.
pub fn worker_caused(runs: &[Attributed]) -> BTreeSet<String> {
    runs.iter().filter(|run| matches!(run.cause, Some(Cause::Kept | Cause::Worker(_)))).map(|run| run.raw.agent.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(overturned: bool, detail: &str) -> Records {
        Records { incidents: vec![Incident { class: "lander-refused".into(), detail: detail.into(), overturned }], ..Records::default() }
    }

    #[test]
    fn a_worker_failure_is_not_excused_by_a_later_lander_mention_or_by_its_own_refused_checks() {
        let report = "Not done: the parser still drops the last field.\n\nThe lander will pick it up once the fix lands.";
        assert_eq!(cause("not done", false, report, &Records::default()), Some(Cause::Worker("missed")));
        let refusal = "Not done: the landing was refused.";
        assert_eq!(cause("not done", false, refusal, &refused(false, "failed test admission")), Some(Cause::Worker("refused")));
        assert_eq!(cause("not done", false, refusal, &refused(true, "failed test admission")), Some(Cause::Harness("gate-overturned")));
        assert_eq!(cause("not done", false, refusal, &refused(true, "failed stale-revert scan")), Some(Cause::Harness("false-revert")));
    }
}
