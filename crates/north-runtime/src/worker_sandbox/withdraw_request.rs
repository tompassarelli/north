//! A queued landing ticket withdrawn through the outside broker for a worker whose domain keeps the landing queue's ticket store unwritable (north#1054): a worker's own `north land withdraw` failed with `lifecycle store: attempt to write a readonly database`. The broker runs `land withdraw TICKET ...` only when every undecided ticket TICKET names, by file name or branch, in the queue of the repository the request names or in its outbox, was queued by the fleet session whose scope the caller's cgroup names (lane_grant.rs), never a session the client names.

use std::path::{Path, PathBuf};
use std::process::Command;

use north_base::land_ticket::{self, Phase};
use north_base::platform::land_queue;

use super::peer::Peer;
use super::lane_grant::{self, Host};

const VALUED: [&str; 4] = ["--reason", "--repo", "--noticed-by", "--class"];

/// The ticket and repository a `land withdraw` request asked from `directory` names, `None` for every other request.
fn asked(arguments: &[String], directory: &Path) -> Option<(String, PathBuf)> {
    let [noun, verb, rest @ ..] = arguments else { return None };
    if noun != "land" || verb != "withdraw" {
        return None;
    }
    let (mut ticket, mut repo) = (String::new(), directory.to_path_buf());
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--repo" => repo = directory.join(words.next().map_or("", String::as_str)),
            flag if VALUED.contains(&flag) => {
                words.next();
            }
            named => named.clone_into(&mut ticket),
        }
    }
    Some((ticket, repo))
}

/// Refuses a `land withdraw` request asked from `directory` unless the session whose scope the caller `peer` runs in on `host` queued every undecided ticket it names; `Ok` for every other request.
pub(super) fn refusal(arguments: &[String], directory: &Path, peer: &Peer, host: &Host) -> Result<(), String> {
    let Some((ticket, repo)) = asked(arguments, directory) else { return Ok(()) };
    let (record_directory, record) = lane_grant::own_session(peer, host, "a ticket is withdrawn through the broker by the fleet session that queued it, and this caller runs in no session's scope")?;
    let session = record["name"].as_str().filter(|name| !name.is_empty()).ok_or_else(|| format!("{} names no session", record_directory.display()))?;
    let queue = land_queue::directory(&state_home(host), &origin(&repo)?);
    let queuers = queuers(&queue, &ticket);
    if queuers.is_empty() {
        return Err(format!("no queued ticket named {ticket} in {}", queue.display()));
    }
    match queuers.into_iter().find(|queuer| queuer.as_deref() != Some(session)) {
        Some(other) => Err(format!("{session} withdraws only a ticket it queued, and {ticket} was queued by {}", other.as_deref().unwrap_or("no session"))),
        None => Ok(()),
    }
}

/// The state home the brokered withdraw reads its queue under: the broker's own, as `request_env::request_homes` gives every verb but a safe-push.
fn state_home(host: &Host) -> PathBuf {
    host.environment.iter().find(|(name, value)| name == "XDG_STATE_HOME" && !value.is_empty()).map_or_else(|| host.home.join(".local/state"), |(_, value)| PathBuf::from(value))
}

fn origin(repo: &Path) -> Result<String, String> {
    let output = Command::new("git").arg("-C").arg(repo).args(["remote", "get-url", "--push", "origin"]).output().map_err(|error| format!("git: {error}"))?;
    let origin = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !origin.is_empty()).then_some(origin).ok_or_else(|| format!("{} has no origin to name a landing queue by", repo.display()))
}

/// The session each undecided ticket `wanted` names in `queue` and its outbox was queued by, `None` for one that names none or cannot be read.
fn queuers(queue: &Path, wanted: &str) -> Vec<Option<String>> {
    let file = if wanted.ends_with(".json") { wanted.to_owned() } else { format!("{wanted}.json") };
    [queue.to_path_buf(), queue.join(land_queue::OUTBOX)]
        .into_iter()
        .flat_map(|directory| {
            let store = land_ticket::recorded(&directory);
            let tickets = std::fs::read_dir(&directory).into_iter().flatten().flatten().map(|entry| entry.path());
            tickets.filter(|path| path.extension().is_some_and(|extension| extension == "json") && path.is_file() && !store.as_ref().and_then(|store| land_ticket::phase(store, &directory, path)).is_some_and(Phase::is_decided)).collect::<Vec<_>>()
        })
        .filter_map(|path| {
            let ticket = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok()).unwrap_or_default();
            let named = path.file_name().is_some_and(|name| name == file.as_str()) || ticket["branch"].as_str() == Some(wanted);
            named.then(|| ticket["session"].as_str().filter(|session| !session.is_empty()).map(str::to_owned))
        })
        .collect()
}
