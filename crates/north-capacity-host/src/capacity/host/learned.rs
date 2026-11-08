//! The capacity-estimate loop's graph side: a finished lease tells the estimate its signature learned, and sizing reads it back through harness/graph-rules/capacity-estimate.json.
use serde_json::Value;

use super::database::{capacity_database, capacity_history};
use super::estimate::{TaskSignature, learned_estimate, learned_json, signature_key};
use super::sizing::own_host;
use crate::capacity::local_machine::host_name;
use crate::foundation::clock::Timestamp;

/// Tells the graph the estimate `signature` learned from its newest runs on this host, in one transaction with the writer's tick.
pub(super) fn tell(signature: &TaskSignature) -> Result<(), String> {
    let history = capacity_database().and_then(|db| capacity_history(&db)).map_err(|error| format!("capacity database: {error}"))?;
    let estimate = learned_estimate(signature, &own_host(history, &host_name())).unwrap_or_default();
    north_graph_loops::tell_estimate(&north_graph_loops::graph_path(), &signature_key(signature), &estimate, Timestamp::now().unix_seconds())
}

/// The estimate the graph holds for `signature`, shaped as sizing reads an estimate.
pub(super) fn read(signature: &TaskSignature) -> Option<Value> {
    let graph = north_graph_loops::graph_path();
    let estimates = graph.exists().then(|| north_graph_loops::estimates(&graph).ok()).flatten()?;
    estimates.get(&signature_key(signature)).filter(|estimate| estimate.contains_key("runs")).map(|estimate| learned_json(signature, estimate))
}
