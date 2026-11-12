//! The check-deadline loop's facts: each passing land check, and each timeout that lane and base both ran past, tells the
//! deadline its check learned, `(check:CLASS:COMMAND :check/deadline-s N)`, and a land check reads its own back by the subject.
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term};

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

/// The live facts about one check, read through the graph's subject index instead of a rule over every fact the land writer told.
fn own_facts(graph: &Graph, key: &str) -> Result<Vec<Fact>, String> {
    let subject = format!("{SUBJECT}{key}");
    let live = graph.show(&Term::string(subject.clone())).map_err(|why| why.to_string())?;
    Ok(live
        .into_iter()
        .filter_map(|live| match live.triple.t2 {
            Term::Atom(Atom::Keyword(predicate)) => Some((subject.clone(), predicate, live.triple.t3)),
            _ => None,
        })
        .collect())
}

/// One check's learned deadline, None when it never told one; where two live values disagree after a race, the longer one stands.
pub fn check_deadline_from(graph: &Graph, key: &str) -> Result<Option<CheckDeadline>, String> {
    let mut learned = None::<CheckDeadline>;
    for (_, predicate, value) in own_facts(graph, key)? {
        let (Some(measure), Some(value)) = (predicate.strip_prefix(PREDICATE), cells::value(&value).and_then(|value| value.as_u64())) else { continue };
        let learned = learned.get_or_insert_default();
        let slot = match measure {
            "deadline-s" => &mut learned.seconds,
            "passes" => &mut learned.passes,
            "p90-s" => learned.p90.get_or_insert(0),
            "exceeded-s" => learned.exceeded.get_or_insert(0),
            _ => continue,
        };
        *slot = (*slot).max(value);
    }
    Ok(learned)
}

pub fn check_deadline(db: &Path, key: &str) -> Result<Option<CheckDeadline>, String> {
    check_deadline_from(&Graph::open_read_only(db, READ_PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?, key)
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
