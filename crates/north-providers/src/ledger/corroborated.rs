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

/// What the graph holds toward the corroborated rule, so a dry run on a host whose rule reads 0 shows which link is empty.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Census {
    pub claims: usize,
    pub landed: usize,
    pub ancestors: usize,
    pub checks_passed: usize,
    pub by_checker: usize,
    pub by_landing: usize,
}

impl std::fmt::Display for Census {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            out,
            "{} claimed commits, {} landed-on, {} ancestor-of origin/main, {} check-passed; corroborated by {} checker and {} lander with git",
            self.claims, self.landed, self.ancestors, self.checks_passed, self.by_checker, self.by_landing
        )
    }
}

pub fn census(graph: &Path) -> NorthResult<Option<Census>> {
    if !graph.is_file() {
        return Ok(None);
    }
    let failed = |why: String| NorthError::Configuration(format!("the corroborated census over {}: {why}", graph.display()));
    let rule = harness_root().map_err(failed).and_then(|root| std::fs::read_to_string(root.join(RULE)).map_err(|why| failed(format!("{RULE}: {why}"))))?;
    let rule: serde_json::Value = serde_json::from_str(&rule).map_err(|why| failed(why.to_string()))?;
    let graph = Graph::open_read_only(graph, PATIENCE).map_err(|why| failed(why.to_string()))?;
    let ask = |query: serde_json::Value| -> NorthResult<Vec<Vec<Term>>> {
        let query = parse_query(&query.to_string()).map_err(|why| failed(why.to_string()))?;
        Ok(graph.ask(&query).map_err(|why| failed(why.to_string()))?.rows)
    };
    let asserted = |writer: &str, predicate: &str| {
        let head = serde_json::json!({"rel": "seen", "args": [{"var": "s"}, {"var": "o"}]});
        let body = serde_json::json!([{"rel": "assertion", "args": [writer, {"var": "s"}, ["keyword", predicate], {"var": "o"}]}]);
        ask(serde_json::json!({"find": "seen", "rules": [{"head": head, "body": body}]})).map(|rows| rows.len())
    };
    let with_find = |find: &str| {
        let mut query = rule.clone();
        query["find"] = find.into();
        query
    };
    let corroborated = ask(with_find("corroborated"))?;
    let by = |writer: &str| corroborated.iter().filter(|row| row.get(2).and_then(string) == Some(writer)).count();
    Ok(Some(Census {
        claims: ask(with_find("claim"))?.len(),
        landed: asserted("lander", "landed-on")?,
        ancestors: asserted("git", "ancestor-of")?,
        checks_passed: asserted(CHECKER, "check-passed")?,
        by_checker: by(CHECKER),
        by_landing: by("lander"),
    }))
}
