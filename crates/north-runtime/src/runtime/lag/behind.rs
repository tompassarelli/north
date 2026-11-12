//! How far behind origin/main each process runs: the seconds since the first landing that changed a runtime input
//! which the runtime it runs lacks, 0 when it holds every one.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct Process {
    pub(super) pid: u32,
    #[serde(default)]
    pub(super) kind: String,
    pub(super) process: String,
    pub(super) runtime: Option<String>,
}

pub(super) struct Seen {
    pub(super) host: String,
    pub(super) processes: Result<Vec<Process>, String>,
}

/// A landing that changed a runtime input, as the pin journal timed it, oldest first.
pub(super) struct Landing {
    pub(super) at: i64,
    pub(super) sha: String,
}

/// One object per process of every host, and one per host that could not be read, with `seconds` null for it.
pub(super) fn objects(seen: &[Seen], landings: &[Landing], now: i64, holds: impl Fn(&str, &str) -> bool) -> Vec<Value> {
    let mut known: BTreeMap<Option<String>, i64> = BTreeMap::new();
    let mut lag = |runtime: Option<&str>| *known.entry(runtime.map(str::to_owned)).or_insert_with(|| seconds(landings, runtime, now, &holds));
    seen.iter()
        .flat_map(|seen| match &seen.processes {
            Ok(processes) => processes.iter().map(|process| json!({"host": seen.host, "kind": process.kind, "process": process.process, "runtime": process.runtime, "seconds": lag(process.runtime.as_deref())})).collect(),
            Err(why) => vec![json!({"host": seen.host, "process": null, "runtime": null, "seconds": null, "unread": why})],
        })
        .collect()
}

/// A runtime that cannot be named is older than the one its link names, so it is timed from the newest landing.
fn seconds(landings: &[Landing], runtime: Option<&str>, now: i64, holds: &impl Fn(&str, &str) -> bool) -> i64 {
    let missed = match runtime {
        Some(runtime) => landings.iter().rev().take_while(|landing| !holds(&landing.sha, runtime)).last(),
        None => landings.last(),
    };
    missed.map_or(0, |landing| (now - landing.at).max(0))
}

/// The worst lag of the readable processes, with the object that has it.
pub(super) fn worst(objects: &[Value]) -> Option<&Value> {
    objects.iter().filter(|object| object["seconds"].is_i64()).max_by_key(|object| object["seconds"].as_i64())
}

/// Whether a process kind is a host service rather than a session's own process.
pub(super) fn serves_host(kind: &str) -> bool {
    !matches!(kind, "session-host" | "session-link" | "tool-server" | "worker-sandbox broker")
}
