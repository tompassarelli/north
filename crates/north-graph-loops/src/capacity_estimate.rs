//! The capacity-estimate loop's facts: each finished lease tells the estimate its command signature learned,
//! `(signature:HASH :capacity/MEASURE value)`, and admission reads it back through harness/graph-rules/capacity-estimate.json.
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use north_graph::{Graph, Term};

use crate::cells;
use crate::facts::{Fact, facts_from, tell_facts_to};

pub const WRITER: &str = "capacity";
const RULE: &str = "harness/graph-rules/capacity-estimate.json";
const SUBJECT: &str = "signature:";
const PREDICATE: &str = "capacity/";
const PATIENCE: Duration = Duration::from_millis(400);

/// What a signature's newest runs measured: the runs behind it, then the p90 of cores, memory, I/O and disk busy share (`-p50` the median) and the p75 wall seconds.
pub const MEASURES: [&str; 14] = [
    "runs", "cores", "cores-p50", "memory-mb", "memory-mb-p50", "io-runs", "io-read-bytes", "io-read-bytes-p50", "io-write-bytes", "io-write-bytes-p50", "disk-busy", "disk-busy-p50", "wall-s", "wall-runs",
];

pub type Estimate = BTreeMap<String, f64>;

/// Each signature's estimate by its hash.
pub type Estimates = BTreeMap<String, Estimate>;

fn fact(signature: &str, measure: &str, value: f64) -> Fact {
    (format!("{SUBJECT}{signature}"), format!("{PREDICATE}{measure}"), Term::float(value))
}

pub fn estimates_from(graph: &Graph) -> Result<Estimates, String> {
    let mut found = Estimates::new();
    for (subject, predicate, value) in facts_from(graph, RULE)? {
        let (Some(signature), Some(measure), Some(value)) = (subject.strip_prefix(SUBJECT), predicate.strip_prefix(PREDICATE), cells::value(&value).and_then(|value| value.as_f64())) else { continue };
        found.entry(signature.to_owned()).or_default().insert(measure.to_owned(), value);
    }
    Ok(found)
}

pub fn estimates(db: &Path) -> Result<Estimates, String> {
    estimates_from(&Graph::open_read_only(db, PATIENCE).map_err(|why| format!("{}: {why}", db.display()))?)
}

/// Tells the estimate `signature` learned as one transaction: its values that moved or went, and the writer's tick at `at`.
pub fn tell_estimate_to(graph: &mut Graph, signature: &str, estimate: &Estimate, at: i64) -> Result<(), String> {
    if let Some(measure) = estimate.keys().find(|measure| !MEASURES.contains(&measure.as_str())) {
        return Err(format!("measure {measure} is outside the capacity vocabulary"));
    }
    let own = format!("{SUBJECT}{signature}");
    let prior: Vec<Fact> = facts_from(graph, RULE)?.into_iter().filter(|(subject, ..)| *subject == own).collect();
    let told: Vec<Fact> = estimate.iter().map(|(measure, value)| fact(signature, measure, *value)).collect();
    tell_facts_to(graph, WRITER, &prior, &told, at)
}

pub fn tell_estimate(db: &Path, signature: &str, estimate: &Estimate, at: i64) -> Result<(), String> {
    tell_estimate_to(&mut Graph::open(db).map_err(|why| format!("{}: {why}", db.display()))?, signature, estimate, at)
}
