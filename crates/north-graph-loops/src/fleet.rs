//! The fleet-retire loop's facts: each reaper tick tells every idle or parked session's verdict, `(session:NAME :fleet/verdict :idle|:parked|:stale)`,
//! and the dead letters of the last day, `(fleet:retire :fleet/dead-letters-24h N)`; the fleet reads them back through harness/graph-rules/fleet-retire.json.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term};

use crate::facts::{Fact, facts_from, tell_facts_to};

pub const WRITER: &str = "fleet";
const RULE: &str = "harness/graph-rules/fleet-retire.json";
const SESSION: &str = "session:";
const RETIRE: &str = "fleet:retire";
const VERDICT: &str = "fleet/verdict";
const DEAD_LETTERS: &str = "fleet/dead-letters-24h";
const PATIENCE: Duration = Duration::from_millis(400);

pub const VERDICTS: [&str; 3] = ["idle", "parked", "stale"];

/// What the reaper's newest tick told: each session's verdict by name and the dead letters of the last day.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Retirement {
    pub verdicts: BTreeMap<String, String>,
    pub dead_letters_24h: Option<i64>,
}

impl Retirement {
    fn facts(&self) -> Vec<Fact> {
        let verdicts = self.verdicts.iter().map(|(name, verdict)| (format!("{SESSION}{name}"), VERDICT.to_owned(), Term::keyword(verdict.clone())));
        verdicts.chain(self.dead_letters_24h.map(|count| (RETIRE.to_owned(), DEAD_LETTERS.to_owned(), Term::int(count)))).collect()
    }
}

pub fn retirement_from(graph: &Graph) -> Result<Retirement, String> {
    let mut found = Retirement::default();
    for (subject, predicate, value) in facts_from(graph, RULE)? {
        match (subject.strip_prefix(SESSION), predicate.as_str(), value) {
            (Some(name), VERDICT, Term::Atom(Atom::Keyword(verdict))) => drop(found.verdicts.insert(name.to_owned(), verdict)),
            (None, DEAD_LETTERS, Term::Atom(Atom::Int(count))) if subject == RETIRE => found.dead_letters_24h = Some(count),
            _ => {}
        }
    }
    Ok(found)
}

pub fn retirement(db: &Path) -> Result<Retirement, String> {
    retirement_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?)
}

/// Tells one reaper tick as one transaction: the sessions whose verdict moved or went, the new ones, the dead letters and the writer's tick at `at`.
pub fn tell_retirement_to(graph: &mut Graph, told: &Retirement, at: i64) -> Result<(), String> {
    if let Some(verdict) = told.verdicts.values().find(|verdict| !VERDICTS.contains(&verdict.as_str())) {
        return Err(format!("verdict {verdict} is outside the fleet vocabulary"));
    }
    let prior = retirement_from(graph)?;
    tell_facts_to(graph, WRITER, &prior.facts(), &told.facts(), at)
}

pub fn tell_retirement(db: &Path, told: &Retirement, at: i64) -> Result<(), String> {
    tell_retirement_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, told, at)
}
