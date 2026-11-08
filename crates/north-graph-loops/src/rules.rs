use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use north_graph::{Action, Graph, Term, Triple, parse_query};

use crate::cells::{self, Cells, Learned, Measure, TICK_AT};

pub(crate) const PATIENCE: Duration = Duration::from_millis(400);
const TICKED: &str = "harness/graph-rules/ticked.json";

pub(crate) fn ask(graph: &Graph, rule: &'static str) -> Result<Vec<Vec<Term>>, String> {
    let query = parse_query(north_base::platform::harness::table(rule)).map_err(|why| format!("{rule}: {why}"))?;
    graph.ask(&query).map(|answer| answer.rows).map_err(|why| format!("{rule}: {why}"))
}

pub(crate) fn open_read_only(db: &Path) -> Result<Graph, String> {
    Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))
}

/// The actions that move a writer's tick instant from `was` to `at`.
fn tick_actions(writer: &str, was: Option<i64>, at: i64) -> Result<Vec<Action>, String> {
    let tick = |seconds| Term::instant(seconds, 0).map(|instant| Triple::new(Term::string(cells::subject(writer)), Term::keyword(TICK_AT), instant)).map_err(|why| why.to_string());
    let mut actions = Vec::new();
    if let Some(was) = was {
        actions.push(Action::retract(tick(was)?));
    }
    actions.push(Action::assert(tick(at)?));
    Ok(actions)
}

/// Each writer's newest tick instant, through harness/graph-rules/ticked.json.
pub fn ticks_from(graph: &Graph) -> Result<BTreeMap<String, i64>, String> {
    Ok(ask(graph, TICKED)?.iter().filter_map(|row| Some((cells::text(row.first()?)?.to_owned(), cells::seconds(row.get(1)?)?))).fold(BTreeMap::new(), |mut ticks, (writer, at)| {
        let newest = ticks.entry(writer).or_insert(at);
        *newest = (*newest).max(at);
        ticks
    }))
}

pub fn ticks(db: &Path) -> Result<BTreeMap<String, i64>, String> {
    ticks_from(&open_read_only(db)?)
}

/// The cells `writer` last told, each read through its measure's rule, and the instant it ticked.
pub fn read_from(graph: &Graph, writer: &str) -> Result<Learned, String> {
    let mut found = Cells::new();
    for measure in Measure::ALL {
        for row in ask(graph, measure.rule())? {
            if let (Some(cell), Some(value)) = (row.first().and_then(cells::text).and_then(|subject| measure.cell(subject)), row.get(1).and_then(cells::value)) {
                found.insert(cell, value);
            }
        }
    }
    Ok(Learned { cells: found, at: ticks_from(graph)?.get(writer).copied() })
}

pub fn read(db: &Path, writer: &str) -> Result<Learned, String> {
    read_from(&open_read_only(db)?, writer)
}

/// Tells one tick as one transaction: retracts each prior value that moved or went, asserts each new
/// one, and moves the writer's tick instant to `at`.
pub fn tell_to(graph: &mut Graph, writer: &str, prior: &Learned, cells: &Cells, at: i64) -> Result<(), String> {
    let triple = |cell: &str, value| cells::triple(cell, value).ok_or_else(|| format!("cell {cell} = {value} is outside the graph's cell vocabulary"));
    let mut actions = Vec::new();
    for (cell, value) in prior.cells.iter().filter(|(cell, value)| cells.get(*cell) != Some(*value)) {
        actions.push(Action::retract(triple(cell, value)?));
    }
    for (cell, value) in cells.iter().filter(|(cell, value)| prior.cells.get(*cell) != Some(*value)) {
        actions.push(Action::assert(triple(cell, value)?));
    }
    actions.extend(tick_actions(writer, prior.at, at)?);
    graph.apply(writer, &actions).map(drop).map_err(|why| why.to_string())
}

pub fn tell(db: &Path, writer: &str, prior: &Learned, cells: &Cells, at: i64) -> Result<(), String> {
    tell_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, writer, prior, cells, at)
}

/// Tells one tick of a writer whose facts are a set of triples: retracts each prior triple the tick no
/// longer holds, asserts each new one, and moves the tick, all in one transaction.
pub(crate) fn tell_set(graph: &mut Graph, writer: &str, prior: &BTreeSet<Triple>, next: &BTreeSet<Triple>, at: i64) -> Result<(), String> {
    let mut actions: Vec<Action> = prior.difference(next).cloned().map(Action::retract).chain(next.difference(prior).cloned().map(Action::assert)).collect();
    actions.extend(tick_actions(writer, ticks_from(graph)?.get(writer).copied(), at)?);
    graph.apply(writer, &actions).map(drop).map_err(|why| why.to_string())
}

pub(crate) fn open_writable(db: &Path) -> Result<Graph, String> {
    Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))
}
