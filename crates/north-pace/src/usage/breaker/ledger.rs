use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::usage::decide::world::{Trip, Usd, Workstream};

pub const FILE: &str = "breaker.jsonl";

#[derive(Deserialize)]
struct Line {
    kind: String,
    scope: String,
    spent: f64,
    p90: f64,
}

/// The workstreams whose latest ledger line is a trip: any later line for the scope (a clear, or the outcome left when its
/// boxes finish) ends it.
pub fn trips(ledger: &str) -> Vec<Trip> {
    let mut standing: BTreeMap<String, Trip> = BTreeMap::new();
    for line in ledger.lines().filter_map(|line| serde_json::from_str::<Line>(line).ok()) {
        let Some(workstream) = line.scope.strip_prefix("workstream:") else { continue };
        if line.kind == "trip" {
            standing.insert(workstream.to_owned(), Trip { workstream: Workstream(workstream.to_owned()), spent: Usd(line.spent), p90: Usd(line.p90) });
        } else {
            standing.remove(workstream);
        }
    }
    standing.into_values().collect()
}

pub fn read(path: &Path) -> Result<Vec<Trip>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(trips(&text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}
