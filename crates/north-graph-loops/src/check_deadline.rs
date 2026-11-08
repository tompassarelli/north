//! The check-deadline loop's facts: each passing land check, and each timeout that lane and base both ran past, tells the
//! deadline its check learned, `(check:CLASS:COMMAND :check/deadline-s N)`, and the lander reads it back through harness/graph-rules/check-deadline.json.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Graph, Term};

use crate::cells;
use crate::facts::{Fact, facts_from, tell_facts_to};

pub const WRITER: &str = "land";
const RULE: &str = "harness/graph-rules/check-deadline.json";
const SUBJECT: &str = "check:";
const PREDICATE: &str = "check/";
const READ_PATIENCE: Duration = Duration::from_millis(400);

/// What a check's recent runs taught: the deadline, the passes behind it, their p90 wall once enough passed, and the timeout lane and base both ran past.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckDeadline {
    pub seconds: u64,
    pub passes: u64,
    pub p90: Option<u64>,
    pub exceeded: Option<u64>,
}

/// Each check's learned deadline by its `CLASS:COMMAND`.
pub type CheckDeadlines = BTreeMap<String, CheckDeadline>;

fn int(value: u64) -> Term {
    Term::int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn facts(key: &str, learned: &CheckDeadline) -> Vec<Fact> {
    let subject = format!("{SUBJECT}{key}");
    let fact = |measure: &str, value: Term| (subject.clone(), format!("{PREDICATE}{measure}"), value);
    let mut told = vec![fact("deadline-s", int(learned.seconds)), fact("passes", int(learned.passes))];
    told.extend(learned.p90.map(|p90| fact("p90-s", int(p90))));
    if let Some(exceeded) = learned.exceeded {
        told.push(fact("exceeded-s", int(exceeded)));
        told.push(fact("timed-out", Term::keyword("true")));
    }
    told
}

pub fn check_key(class: &str, command: &str) -> String {
    format!("{class}:{command}")
}

/// Every check's deadline; where two live values disagree after a race, the longer one stands.
pub fn check_deadlines_from(graph: &Graph) -> Result<CheckDeadlines, String> {
    let mut found = CheckDeadlines::new();
    for (subject, predicate, value) in facts_from(graph, RULE)? {
        let (Some(key), Some(measure), Some(value)) = (subject.strip_prefix(SUBJECT), predicate.strip_prefix(PREDICATE), cells::value(&value).and_then(|value| value.as_u64())) else { continue };
        let learned = found.entry(key.to_owned()).or_default();
        let slot = match measure {
            "deadline-s" => &mut learned.seconds,
            "passes" => &mut learned.passes,
            "p90-s" => learned.p90.get_or_insert(0),
            "exceeded-s" => learned.exceeded.get_or_insert(0),
            _ => continue,
        };
        *slot = (*slot).max(value);
    }
    Ok(found)
}

pub fn check_deadlines(db: &Path) -> Result<CheckDeadlines, String> {
    check_deadlines_from(&Graph::open_read_only(db, READ_PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?)
}

/// Tells one check's deadline as one transaction: its values that moved or went, and the lander's tick at `at`.
pub fn tell_check_deadline_to(graph: &mut Graph, key: &str, learned: &CheckDeadline, at: i64) -> Result<(), String> {
    let own = format!("{SUBJECT}{key}");
    let prior: Vec<Fact> = facts_from(graph, RULE)?.into_iter().filter(|(subject, ..)| *subject == own).collect();
    tell_facts_to(graph, WRITER, &prior, &facts(key, learned), at)
}

/// A land check waits no longer than `patience` for the graph's write lock, so an ingest never slows a landing; the next pass of the check tells again.
pub fn tell_check_deadline(db: &Path, key: &str, learned: &CheckDeadline, at: i64, patience: Duration) -> Result<(), String> {
    tell_check_deadline_to(&mut Graph::open_patiently(db, patience).map_err(|why| format!("{}: {why}", db.display()))?, key, learned, at)
}
