//! How far behind the settling host's pin each process runs: the seconds since the promotion that made the pin live,
//! 0 when the process runs the pin or a runtime built on it (docs/rationale/propagation.md, Promotion cadence).
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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

/// The runtime the settling host pinned, and when its promotion made it live.
pub(super) struct Pin {
    pub(super) at: i64,
    pub(super) sha: String,
}

/// One object per process of every host, and one per host that could not be read, with `seconds` null for it.
pub(super) fn objects(seen: &[Seen], pin: Option<&Pin>, now: i64, holds: impl Fn(&str, &str) -> bool) -> Vec<Value> {
    seen.iter()
        .flat_map(|seen| match &seen.processes {
            Ok(processes) => processes.iter().map(|process| json!({"host": seen.host, "kind": process.kind, "process": process.process, "runtime": process.runtime, "seconds": seconds(pin, process.runtime.as_deref(), now, &holds)})).collect(),
            Err(why) => vec![json!({"host": seen.host, "process": null, "runtime": null, "seconds": null, "unread": why})],
        })
        .collect()
}

/// A runtime that cannot be named is older than the one its link names, so it is older than the pin.
fn seconds(pin: Option<&Pin>, runtime: Option<&str>, now: i64, holds: &impl Fn(&str, &str) -> bool) -> i64 {
    match pin {
        Some(pin) if !runtime.is_some_and(|runtime| holds(&pin.sha, runtime)) => (now - pin.at).max(0),
        _ => 0,
    }
}

/// The worst lag of the readable processes, with the object that has it.
pub(super) fn worst(objects: &[Value]) -> Option<&Value> {
    objects.iter().filter(|object| object["seconds"].is_i64()).max_by_key(|object| object["seconds"].as_i64())
}

/// Whether a process kind is a host service rather than a session's own process.
pub(super) fn serves_host(kind: &str) -> bool {
    !matches!(kind, "session-host" | "session-link" | "tool-server" | "worker-sandbox broker")
}
