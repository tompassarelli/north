use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term, parse_query};

use crate::error::{NorthError, NorthResult};
use crate::platform::harness::harness_root;

const RULE: &str = "harness/graph-rules/corroborated.json";
const CHECKER: &str = "checker";
const PATIENCE: Duration = Duration::from_secs(5);

pub fn checked_boxes(graph: &Path) -> NorthResult<Option<HashSet<String>>> {
    if !graph.is_file() {
        return Ok(None);
    }
    let failed = |why: String| NorthError::Configuration(format!("the corroborated rule over {}: {why}", graph.display()));
    let rule = harness_root().map_err(failed).and_then(|root| std::fs::read_to_string(root.join(RULE)).map_err(|why| failed(format!("{RULE}: {why}"))))?;
    let query = parse_query(&rule).map_err(|why| failed(why.to_string()))?;
    let graph = Graph::open_read_only(graph, PATIENCE).map_err(|why| failed(why.to_string()))?;
    let answer = graph.ask(&query).map_err(|why| failed(why.to_string()))?;
    Ok(Some(answer.rows.iter().filter(|row| row.get(2).and_then(string) == Some(CHECKER)).filter_map(|row| row.first().and_then(string).map(str::to_owned)).collect()))
}

fn string(term: &Term) -> Option<&str> {
    match term {
        Term::Atom(Atom::String(value)) => Some(value),
        _ => None,
    }
}
