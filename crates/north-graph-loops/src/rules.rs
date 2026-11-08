use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Action, Graph, Term, parse_query};

use crate::cells::{self, Cells, Learned, Measure, TICK_AT};

const PATIENCE: Duration = Duration::from_millis(400);
const TICKED: &str = "harness/graph-rules/ticked.json";

fn ask(graph: &Graph, rule: &'static str) -> Result<Vec<Vec<Term>>, String> {
    let query = parse_query(north_base::platform::harness::table(rule)).map_err(|why| format!("{rule}: {why}"))?;
    graph.ask(&query).map(|answer| answer.rows).map_err(|why| format!("{rule}: {why}"))
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
    ticks_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?)
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
    read_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?, writer)
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
    let tick = |seconds| Term::instant(seconds, 0).map(|instant| north_graph::Triple::new(Term::string(cells::subject(writer)), Term::keyword(TICK_AT), instant)).map_err(|why| why.to_string());
    if let Some(was) = prior.at {
        actions.push(Action::retract(tick(was)?));
    }
    actions.push(Action::assert(tick(at)?));
    graph.apply(writer, &actions).map(drop).map_err(|why| why.to_string())
}

pub fn tell(db: &Path, writer: &str, prior: &Learned, cells: &Cells, at: i64) -> Result<(), String> {
    tell_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, writer, prior, cells, at)
}
