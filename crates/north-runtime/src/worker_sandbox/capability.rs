//! The capability record of the fleet session a process belongs to: the record its launcher wrote at launch, in the fleet's state, which no worker writes, found from what the kernel says of the caller (the session scope or held unit its cgroup names, or the adopted window it descends from: lane_grant::own_session), never from a variable or word the caller sets. Every confinement decision reads it, so a worker that sets `NORTH_FLEET_ROLE=coordinator` before asking the broker for a build output run is still confined (north#1574).

use std::path::Path;

use north_base::platform::session_scope;
use serde_json::Value;

use super::lane_grant::{self, Host};
use super::peer::Peer;
use super::{BUILT, FLAG, PROGRAM, WORKER};
use crate::fleet::model::Role;

const NO_SESSION: &str = "";

/// The role a session record names, or the one the fleet reads from a record that names none (`Role::unnamed`: a worker's for a record with a parent or a task, the coordinator seat's by its name, else a lead's), since the field is optional and older records omit it.
pub(super) fn recorded_role(record: &Value) -> String {
    record["role"].as_str().map_or_else(|| Role::unnamed(!record["parent"].is_null() || !record["task_ref"].is_null(), record["name"].as_str().unwrap_or_default()).label().to_owned(), str::to_owned)
}

/// Whether the caller `peer` on `host` belongs to a fleet session whose record confines it: every session but the coordinator seat (`session_scope::confined_role`). A caller in a session's unit whose record cannot be read is confined, and a caller in no session's unit or window is the operator's own process.
pub(super) fn confines(peer: &Peer, host: &Host) -> bool {
    match lane_grant::own_session(peer, host, NO_SESSION) {
        Ok((_, record)) => session_scope::confined_role(Some(&recorded_role(&record))),
        Err(why) => why != NO_SESSION,
    }
}

/// Whether this process is confined: when the broker started it with `--worker` (`marked`), when its own session's record says so ([`confines`]), or when its environment names a confined role; the word and the role can only narrow, so a launch that started a worker outside its session's unit keeps it confined.
pub(super) fn confines_here(marked: bool) -> bool {
    marked || session_scope::confined_session() || confines(&Peer::of(std::process::id()), &Host::own())
}

/// Where the command begins in the arguments of `capacity host run`: after the first `--`, when a word follows it.
pub(super) fn payload(words: &[&str]) -> Option<usize> {
    words.iter().position(|word| *word == "--").map(|at| at + 1).filter(|start| *start < words.len())
}

/// `arguments` as the broker starts them: the gate's command wrapped to start in a new domain, since the gate runs outside the client's domain and the command must not, and that domain and a build output run's a worker's when `worker` says the caller's record confines it ([`confines`]), never because of a variable the caller set.
pub(super) fn confined(north: &Path, arguments: Vec<String>, worker: bool) -> Vec<String> {
    let marked = worker.then(|| WORKER.to_owned());
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    if let [FLAG, BUILT, "--", ..] = words.as_slice() {
        return arguments[..2].iter().cloned().chain(marked).chain(arguments[2..].iter().cloned()).collect();
    }
    let ["capacity", "host", "run", rest @ ..] = words.as_slice() else { return arguments };
    let Some(start) = payload(rest) else { return arguments };
    let wrapper = [north.to_string_lossy().into_owned(), FLAG.to_owned(), PROGRAM.to_owned()].into_iter().chain(marked).chain(["--".to_owned()]);
    arguments[..3 + start].iter().cloned().chain(wrapper).chain(arguments[3 + start..].iter().cloned()).collect()
}

/// The words of `north --worker-sandbox` without the broker's `--worker`, and whether they carried it: before a shell command, or after `--built` or `--program`, the only places [`confined`] and the canary put it.
pub(super) fn worker_word(arguments: &[String]) -> (bool, Vec<String>) {
    let at = match arguments {
        [word, _] if word == WORKER => 0,
        [form, word, ..] if (form == BUILT || form == PROGRAM) && word == WORKER => 1,
        _ => return (false, arguments.to_vec()),
    };
    (true, arguments.iter().enumerate().filter(|(index, _)| *index != at).map(|(_, word)| word.clone()).collect())
}

#[cfg(test)]
#[path = "capability_record_tests.rs"]
mod capability_record_tests;
