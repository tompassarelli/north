//! The lander-doctor loop's facts: each run tells what the doctor remembers, `(bound:KEY :doctor/since N)`, `:doctor/remedy "AT CLASS"`,
//! `:doctor/paged N` and `:doctor/state :ok|:over|:unknown` per bound, `(lander:BUILD :doctor/good RANK)` per build that landed green and
//! `(incident:CLASS :doctor/count N)`; the doctor reads them back through harness/graph-rules/lander-doctor.json.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term};

use crate::cells;
use crate::facts::{Fact, facts_from, tell_facts_to};

pub const DOCTOR_WRITER: &str = "land-doctor";
const RULE: &str = "harness/graph-rules/lander-doctor.json";
const BOUND: &str = "bound:";
const BUILD: &str = "lander:";
const INCIDENT: &str = "incident:";
const READ_PATIENCE: Duration = Duration::from_millis(400);

/// One bound held over from `since`: each remedy applied in it as `(at, class)` and the instant the proxy was paged about it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Held {
    pub since: u64,
    pub remedies: Vec<(u64, String)>,
    pub paged: Option<u64>,
}

/// What the doctor remembers between runs, and the instant it last ran.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Remembered {
    pub good: Vec<String>,
    pub episodes: BTreeMap<String, Held>,
    pub states: BTreeMap<String, String>,
    pub incidents: BTreeMap<String, u64>,
    pub at: Option<i64>,
}

fn int(value: u64) -> Term {
    Term::int(i64::try_from(value).unwrap_or(i64::MAX))
}

fn number(term: &Term) -> Option<u64> {
    cells::value(term)?.as_u64()
}

fn facts(memory: &Remembered) -> Vec<Fact> {
    let mut told = Vec::new();
    for (rank, build) in memory.good.iter().enumerate() {
        told.push((format!("{BUILD}{build}"), "doctor/good".to_owned(), int(rank as u64)));
    }
    for (key, state) in &memory.states {
        told.push((format!("{BOUND}{key}"), "doctor/state".to_owned(), Term::keyword(state.clone())));
    }
    for (key, held) in &memory.episodes {
        let subject = format!("{BOUND}{key}");
        told.push((subject.clone(), "doctor/since".to_owned(), int(held.since)));
        told.extend(held.remedies.iter().map(|(at, class)| (subject.clone(), "doctor/remedy".to_owned(), Term::string(format!("{at} {class}")))));
        told.extend(held.paged.map(|at| (subject.clone(), "doctor/paged".to_owned(), int(at))));
    }
    told.extend(memory.incidents.iter().map(|(class, count)| (format!("{INCIDENT}{class}"), "doctor/count".to_owned(), int(*count))));
    told
}

fn own(subject: &str) -> bool {
    [BOUND, BUILD, INCIDENT].iter().any(|prefix| subject.starts_with(prefix))
}

pub fn remembered_from(graph: &Graph) -> Result<Remembered, String> {
    let mut memory = Remembered::default();
    let mut ranked: Vec<(u64, String)> = Vec::new();
    for (subject, predicate, value) in facts_from(graph, RULE)? {
        if let Some(key) = subject.strip_prefix(BOUND) {
            match (predicate.as_str(), &value) {
                ("doctor/state", Term::Atom(Atom::Keyword(state))) => drop(memory.states.insert(key.to_owned(), state.clone())),
                ("doctor/since", value) => memory.episodes.entry(key.to_owned()).or_default().since = number(value).unwrap_or_default(),
                ("doctor/paged", value) => memory.episodes.entry(key.to_owned()).or_default().paged = number(value),
                ("doctor/remedy", value) => {
                    let Some((at, class)) = cells::text(value).and_then(|text| text.split_once(' ')) else { continue };
                    memory.episodes.entry(key.to_owned()).or_default().remedies.push((at.parse().unwrap_or_default(), class.to_owned()));
                }
                _ => {}
            }
        } else if let (Some(build), Some(rank)) = (subject.strip_prefix(BUILD), number(&value)) {
            ranked.push((rank, build.to_owned()));
        } else if let (Some(class), Some(count)) = (subject.strip_prefix(INCIDENT), number(&value)) {
            memory.incidents.insert(class.to_owned(), count);
        }
    }
    ranked.sort();
    memory.good = ranked.into_iter().map(|(_, build)| build).collect();
    memory.episodes.values_mut().for_each(|held| held.remedies.sort());
    memory.at = crate::rules::ticks_from(graph)?.get(DOCTOR_WRITER).copied();
    Ok(memory)
}

/// What the doctor last told, or nothing when the graph does not exist yet.
pub fn remembered(db: &Path) -> Result<Option<Remembered>, String> {
    if !db.exists() {
        return Ok(None);
    }
    remembered_from(&Graph::open_read_only(db, READ_PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?).map(Some)
}

/// Tells one run as one transaction: what moved or went since the last run, and the doctor's tick at `at`.
pub fn tell_remembered_to(graph: &mut Graph, memory: &Remembered, at: i64) -> Result<(), String> {
    let prior: Vec<Fact> = facts_from(graph, RULE)?.into_iter().filter(|(subject, ..)| own(subject)).collect();
    tell_facts_to(graph, DOCTOR_WRITER, &prior, &facts(memory), at)
}

pub fn tell_remembered(db: &Path, memory: &Remembered, at: i64) -> Result<(), String> {
    tell_remembered_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, memory, at)
}
