//! The capacity-estimate loop's facts: each finished lease tells the estimate its command signature learned,
//! `(signature:HASH :capacity/MEASURE value)`, and admission reads back that one signature's facts by its subject,
//! so a grant costs one indexed lookup rather than a scan of every live assertion in the graph.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Atom, Graph, Term, parse_query};
use serde_json::json;

use crate::cells;
use crate::facts::{Fact, tell_facts_to};

pub const WRITER: &str = "capacity";
const SUBJECT: &str = "signature:";
const PREDICATE: &str = "capacity/";
const PATIENCE: Duration = Duration::from_millis(400);

/// What a signature's newest runs measured: the runs behind it, then the p90 of cores, memory, I/O and disk busy share (`-p50` the median) and the p75 wall seconds.
pub const MEASURES: [&str; 14] = [
    "runs", "cores", "cores-p50", "memory-mb", "memory-mb-p50", "io-runs", "io-read-bytes", "io-read-bytes-p50", "io-write-bytes", "io-write-bytes-p50", "disk-busy", "disk-busy-p50", "wall-s", "wall-runs",
];

pub type Estimate = BTreeMap<String, f64>;

fn fact(signature: &str, measure: &str, value: f64) -> Fact {
    (format!("{SUBJECT}{signature}"), format!("{PREDICATE}{measure}"), Term::float(value))
}

/// The writer's facts about `signature`, asked with its subject bound so the store reads them through its subject index.
fn signature_facts(graph: &Graph, signature: &str) -> Result<Vec<Fact>, String> {
    let subject = format!("{SUBJECT}{signature}");
    let rule = json!({"head": {"rel": "capacity_estimate", "args": [{"var": "predicate"}, {"var": "value"}]}, "body": [
        {"rel": "assertion", "args": [WRITER, subject, {"var": "predicate"}, {"var": "value"}]}]});
    let query = parse_query(&json!({"find": "capacity_estimate", "rules": [rule]}).to_string()).map_err(|why| format!("capacity estimate query: {why}"))?;
    let answer = graph.ask(&query).map_err(|why| format!("capacity estimate query: {why}"))?;
    Ok(answer
        .rows
        .iter()
        .filter_map(|row| match row.as_slice() {
            [Term::Atom(Atom::Keyword(predicate)), value] => Some((subject.clone(), predicate.clone(), value.clone())),
            _ => None,
        })
        .collect())
}

/// The estimate `signature` learned, empty when no lease has told one.
pub fn estimate_from(graph: &Graph, signature: &str) -> Result<Estimate, String> {
    Ok(signature_facts(graph, signature)?
        .into_iter()
        .filter_map(|(_, predicate, value)| Some((predicate.strip_prefix(PREDICATE)?.to_owned(), cells::value(&value)?.as_f64()?)))
        .collect())
}

pub fn estimate(db: &Path, signature: &str) -> Result<Estimate, String> {
    estimate_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?, signature)
}

/// Tells the estimate `signature` learned as one transaction: its values that moved or went, and the writer's tick at `at`.
pub fn tell_estimate_to(graph: &mut Graph, signature: &str, estimate: &Estimate, at: i64) -> Result<(), String> {
    if let Some(measure) = estimate.keys().find(|measure| !MEASURES.contains(&measure.as_str())) {
        return Err(format!("measure {measure} is outside the capacity vocabulary"));
    }
    let prior = signature_facts(graph, signature)?;
    let told: Vec<Fact> = estimate.iter().map(|(measure, value)| fact(signature, measure, *value)).collect();
    tell_facts_to(graph, WRITER, &prior, &told, at)
}

pub fn tell_estimate(db: &Path, signature: &str, estimate: &Estimate, at: i64) -> Result<(), String> {
    tell_estimate_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, signature, estimate, at)
}
