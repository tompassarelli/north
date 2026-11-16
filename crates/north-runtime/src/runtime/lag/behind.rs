//! How long each process has stayed behind its host: the seconds since the host's runtime link swapped to a runtime
//! the process does not run, 0 when it runs the linked runtime or one built on it (docs/rationale/propagation.md).
use crate::runtime::live::Swap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(super) struct Process {
    pub(super) pid: u32,
    #[serde(default)]
    pub(super) kind: String,
    pub(super) process: String,
    pub(super) runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) leaving_until: Option<u64>,
}

pub(super) struct Seen {
    pub(super) host: String,
    pub(super) swap: Option<Swap>,
    pub(super) processes: Result<Vec<Process>, String>,
}

/// One object per process of every host, and one per host that could not be read, with `seconds` null for it; a host a recorded hold keeps reads 0 and names the hold.
pub(super) fn objects(seen: &[Seen], now: i64, built_on: impl Fn(&str, &str) -> bool, held: impl Fn(&Seen) -> Option<String>) -> Vec<Value> {
    seen.iter()
        .flat_map(|seen| match &seen.processes {
            Ok(processes) => {
                let hold = held(seen);
                processes.iter().map(|process| {
                    let leaving = process.leaving_until.is_some_and(|until| i64::try_from(until).is_ok_and(|until| until > now));
                    let behind = if hold.is_some() || leaving { 0 } else { behind_since_its_host_took(seen.swap.as_ref(), process.runtime.as_deref(), now, &built_on) };
                    let mut object = json!({"host": seen.host, "kind": process.kind, "process": process.process, "runtime": process.runtime, "linked": seen.swap.as_ref().map(|swap| &swap.runtime), "seconds": behind});
                    if let Some(hold) = &hold {
                        object["held"] = Value::Bool(true);
                        object["hold"] = Value::from(hold.as_str());
                    }
                    object
                }).collect()
            }
            Err(why) => vec![json!({"host": seen.host, "process": null, "runtime": null, "seconds": null, "unread": why})],
        })
        .collect()
}

/// A runtime that cannot be named is older than the one its link names, so it is behind since the link swapped.
fn behind_since_its_host_took(swap: Option<&Swap>, runtime: Option<&str>, now: i64, built_on: &impl Fn(&str, &str) -> bool) -> i64 {
    match swap {
        Some(swap) if !runtime.is_some_and(|runtime| built_on(&swap.runtime, runtime)) => (now - swap.at).max(0),
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
