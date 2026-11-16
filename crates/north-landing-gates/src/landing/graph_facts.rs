use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use north_graph::{Action, Atom, Graph, Term, Triple, parse_query};
use serde_json::Value;

pub const PATIENCE: Duration = Duration::from_millis(400);
const TICKED: &str = "harness/graph-rules/ticked.json";
const TICK_AT: &str = "loop/tick-at";

/// One asserted triple of a writer: its subject, its predicate and its value.
pub type Fact = (String, String, Term);

pub fn ask(graph: &Graph, rule: &'static str) -> Result<Vec<Vec<Term>>, String> {
    let query = parse_query(north_base::platform::harness::table(rule)).map_err(|why| format!("{rule}: {why}"))?;
    graph.ask(&query).map(|answer| answer.rows).map_err(|why| format!("{rule}: {why}"))
}

pub fn open_read_only(db: &Path) -> Result<Graph, String> {
    Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))
}

pub fn open_writable(db: &Path) -> Result<Graph, String> {
    Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))
}

fn tick_actions(writer: &str, was: Option<i64>, at: i64) -> Result<Vec<Action>, String> {
    let tick = |seconds| Term::instant(seconds, 0).map(|instant| Triple::new(Term::string(format!("loop:{writer}")), Term::keyword(TICK_AT), instant)).map_err(|why| why.to_string());
    let mut actions = Vec::new();
    if let Some(was) = was {
        actions.push(Action::retract(tick(was)?));
    }
    actions.push(Action::assert(tick(at)?));
    Ok(actions)
}

/// Each writer's newest tick instant, through harness/graph-rules/ticked.json.
pub fn ticks_from(graph: &Graph) -> Result<BTreeMap<String, i64>, String> {
    Ok(ask(graph, TICKED)?.iter().filter_map(|row| Some((text(row.first()?)?.to_owned(), seconds(row.get(1)?)?))).fold(BTreeMap::new(), |mut ticks, (writer, at)| {
        let newest = ticks.entry(writer).or_insert(at);
        *newest = (*newest).max(at);
        ticks
    }))
}

/// Tells one tick of a writer whose facts are a set of triples: retracts each prior triple the tick no longer holds, asserts each new one, and moves the tick, all in one transaction.
pub fn tell_set(graph: &mut Graph, writer: &str, prior: &BTreeSet<Triple>, next: &BTreeSet<Triple>, at: i64) -> Result<(), String> {
    let mut actions: Vec<Action> = prior.difference(next).cloned().map(Action::retract).chain(next.difference(prior).cloned().map(Action::assert)).collect();
    actions.extend(tick_actions(writer, ticks_from(graph)?.get(writer).copied(), at)?);
    graph.apply(writer, &actions).map(drop).map_err(|why| why.to_string())
}

/// Tells one tick as one transaction: retracts each prior fact that moved or went, asserts each new one, and moves the writer's tick to `at`.
pub fn tell_facts_to(graph: &mut Graph, writer: &str, prior: &[Fact], facts: &[Fact], at: i64) -> Result<(), String> {
    let triples = |facts: &[Fact]| facts.iter().map(|(subject, predicate, value)| Triple::new(Term::string(subject.clone()), Term::keyword(predicate.clone()), value.clone())).collect::<BTreeSet<_>>();
    tell_set(graph, writer, &triples(prior), &triples(facts), at)
}

pub fn value(term: &Term) -> Option<Value> {
    match term {
        Term::Atom(Atom::String(text)) => Some(Value::from(text.clone())),
        Term::Atom(Atom::Int(number)) => Some(Value::from(*number)),
        Term::Atom(Atom::Float(number)) => Some(Value::from(number.value())),
        _ => None,
    }
}

pub fn text(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(Atom::String(text)) => Some(text),
        _ => None,
    }
}

fn seconds(term: &Term) -> Option<i64> {
    match term {
        Term::Atom(Atom::Instant(instant)) => Some(instant.seconds()),
        _ => None,
    }
}
