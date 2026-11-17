//! A queued landing ticket withdrawn through the outside broker (north#1054, north#1650): a confined session's domain keeps the landing queue's ticket store unwritable, and every `north --worker-sandbox` shell routes `north land withdraw` through the broker. A confined session withdraws only undecided tickets queued by its own session (lane_grant.rs); the coordinator seat and a plain session or operator login outside every confined fleet unit, broker unit and capacity lease scope may withdraw any undecided ticket in the repository's queue or outbox. The queue is named by the push address of origin in the project's trusted mirror, which no worker writes, never by the checkout's.

use std::path::{Path, PathBuf};

use north_base::land_ticket::{self, Phase};
use north_base::platform::{land_queue, session_scope};
use north_trusted_mirror::Mirror;

use super::lane_grant::{self, Host};
use super::peer::Peer;
use super::request_checkout;

const VALUED: [&str; 3] = ["--reason", "--noticed-by", "--class"];

fn inline_flag(word: &str, directory: &Path, repo: &mut PathBuf) -> Option<Result<(), String>> {
    let (flag, value) = word.split_once('=').filter(|(flag, _)| flag.starts_with("--"))?;
    match flag {
        "--repo" => *repo = directory.join(value),
        _ if VALUED.contains(&flag) => {}
        other => return Some(Err(format!("land withdraw takes no {other}"))),
    }
    Some(Ok(()))
}

fn parse_word<'a>(word: &str, words: &mut std::slice::Iter<'a, String>, directory: &Path, ticket: &mut String, repo: &mut PathBuf) -> Result<(), String> {
    if let Some(inline) = inline_flag(word, directory, repo) {
        return inline;
    }
    match word {
        "--repo" => *repo = directory.join(words.next().ok_or_else(|| "land withdraw --repo needs a directory".to_owned())?),
        flag if VALUED.contains(&flag) => {
            words.next();
        }
        flag if flag.starts_with('-') => return Err(format!("land withdraw takes no {flag}")),
        named => named.clone_into(ticket),
    }
    Ok(())
}

fn asked(arguments: &[String], directory: &Path) -> Option<Result<(String, PathBuf), String>> {
    let [noun, verb, rest @ ..] = arguments else { return None };
    if noun != "land" || verb != "withdraw" {
        return None;
    }
    let (mut ticket, mut repo) = (String::new(), directory.to_path_buf());
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if let Err(error) = parse_word(word, &mut words, directory, &mut ticket, &mut repo) {
            return Some(Err(error));
        }
    }
    Some(Ok((ticket, repo)))
}

fn is_coordinator_held(cgroup: &str) -> bool {
    let prefix = format!("north-fleet-{}-", session_scope::COORDINATOR_ROLE);
    cgroup.lines().flat_map(|line| line.split('/')).any(|unit| {
        unit.strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".service"))
            .is_some_and(|id| id.len() == 8 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
    })
}

pub(super) fn refusal(arguments: &[String], directory: &Path, peer: &Peer, host: &Host) -> Result<(), String> {
    let Some(asked) = asked(arguments, directory) else { return Ok(()) };
    let (ticket, repo) = asked?;
    let Some(cgroup) = peer.cgroup() else {
        return Err("a caller whose cgroup cannot be read does not withdraw a ticket".to_owned());
    };
    if session_scope::lane_only_cgroup(cgroup) {
        return Err("a lane-only session's command does not reach a ledger through the broker".to_owned());
    }
    let caller = match lane_grant::own_session(peer, host, "") {
        Ok((record_directory, record)) => {
            let session = record["name"].as_str().filter(|name| !name.is_empty()).ok_or_else(|| format!("{} names no session", record_directory.display()))?;
            session_scope::confined_role(Some(&super::capability::recorded_role(&record))).then(|| session.to_owned())
        }
        Err(why) if !why.is_empty() => return Err(why),
        Err(_) if session_scope::fleet_session_cgroup(cgroup) && !is_coordinator_held(cgroup) => {
            return Err("this caller runs in a confined fleet or broker unit outside a recorded session scope".to_owned());
        }
        Err(_) if north_capacity::capacity::gate::lease_cgroup(cgroup).is_some() => {
            return Err("run north land withdraw from the session itself, not inside a capacity lease scope".to_owned());
        }
        Err(_) => None,
    };
    let queue = land_queue::directory(&state_home(host), &origin(host, &repo)?);
    let queuers = queuers(&queue, &ticket);
    if queuers.is_empty() {
        return Err(format!("no queued ticket named {ticket} in {}", queue.display()));
    }
    let Some(session) = caller else { return Ok(()) };
    match queuers.into_iter().find(|queuer| queuer.as_deref() != Some(session.as_str())) {
        Some(other) => Err(format!("{session} withdraws only a ticket it queued, and {ticket} was queued by {}", other.as_deref().unwrap_or("no session"))),
        None => Ok(()),
    }
}

fn state_home(host: &Host) -> PathBuf {
    host.environment.iter().find(|(name, value)| name == "XDG_STATE_HOME" && !value.is_empty()).map_or_else(|| host.home.join(".local/state"), |(_, value)| PathBuf::from(value))
}

fn origin(host: &Host, repo: &Path) -> Result<String, String> {
    let (project, _) = request_checkout::project(&host.code, repo).ok_or_else(|| format!("{} is in no project of this host's to name a landing queue by", repo.display()))?;
    Mirror::existing(&project.join("main"))?.push_address()
}

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
