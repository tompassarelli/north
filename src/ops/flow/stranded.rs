//! The lanes stopped fleet sessions left unlanded, from `north fleet stranded --json`: a
//! repository's row lists each one that has waited fifteen minutes, with its stop, and the stall
//! alert names them until dispatch revives or forgets their sessions.
use std::time::Duration;

use serde_json::{Value, json};

use super::host::Host;
use crate::fleet::stranded::Stranded;

const AFTER_S: f64 = 900.0;
const LIMIT: Duration = Duration::from_secs(120);

pub(crate) fn read(host: &dyn Host) -> Vec<Stranded> {
    let Some(output) = host.output(&["north", "fleet", "stranded", "--json"], None, None, LIMIT).filter(|output| output.status.success()) else { return Vec::new() };
    serde_json::from_slice(&output.stdout).unwrap_or_default()
}

/// The repository's unlanded lanes whose sessions stopped at least fifteen minutes ago.
pub(crate) fn rows(stranded: &[Stranded], repo: &str, now: f64) -> Value {
    let rows = stranded
        .iter()
        .filter(|session| session.repo.as_deref() == Some(repo))
        .filter(|session| now - session.ended_ms as f64 / 1000.0 >= AFTER_S)
        .flat_map(|session| {
            let minutes = (now - session.ended_ms as f64 / 1000.0) / 60.0;
            session.lanes.iter().filter(|lane| lane.unlanded()).map(move |lane| json!({"session": session.name, "lane": lane.label(), "stop": session.stop.label(), "machine": session.machine, "minutes": minutes}))
        })
        .collect();
    Value::Array(rows)
}

/// `n-b245b n245-citations (Stop: capacity, 190m)` for each lane the row lists.
pub(crate) fn lines(row: &Value) -> Vec<String> {
    row["stranded"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|lane| format!("{} {} ({}, {:.0}m)", lane["session"].as_str().unwrap_or_default(), lane["lane"].as_str().unwrap_or_default(), lane["stop"].as_str().unwrap_or_default(), lane["minutes"].as_f64().unwrap_or(0.0)))
        .collect()
}
