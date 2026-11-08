use std::collections::BTreeSet;
use std::path::Path;

use north_graph::{Graph, Term, Triple};

use crate::cells;
use crate::rules::{self, PATIENCE};

pub const WRITER: &str = "distill";
const RULE: &str = "harness/graph-rules/corrections.json";
const SEEN_AT: &str = "corrections/seen-at";

fn triple(class: &str, at: i64) -> Result<Triple, String> {
    Ok(Triple::new(Term::string(format!("correction-class:{class}")), Term::keyword(SEEN_AT), Term::instant(at, 0).map_err(|why| why.to_string())?))
}

/// Each correction's class and instant in unix seconds, as `distill` last told them.
pub fn seen_from(graph: &Graph) -> Result<Vec<(String, i64)>, String> {
    Ok(rules::ask(graph, RULE)?.iter().filter_map(|row| Some((row.first().and_then(cells::text)?.strip_prefix("correction-class:")?.to_owned(), cells::seconds(row.get(1)?)?))).collect())
}

pub fn seen(db: &Path) -> Result<Vec<(String, f64)>, String> {
    let graph = Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?;
    Ok(seen_from(&graph)?.into_iter().map(|(class, at)| (class, at as f64)).collect())
}

pub fn tell_to(graph: &mut Graph, seen: &[(String, i64)], at: i64) -> Result<(), String> {
    let set = |seen: &[(String, i64)]| seen.iter().map(|(class, at)| triple(class, *at)).collect::<Result<BTreeSet<_>, _>>();
    rules::tell_set(graph, WRITER, &set(&seen_from(graph)?)?, &set(seen)?, at)
}

/// Tells one distill run: every correction in the ledger by class and instant, and the tick.
pub fn tell(db: &Path, seen: &[(String, i64)], at: i64) -> Result<(), String> {
    tell_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, seen, at)
}
