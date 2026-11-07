//! The landing tickets a session enqueued that have not landed, which hold the session listed.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use north_base::land_ticket::{self, Phase};
use serde_json::Value;

use crate::error::NorthResult;

pub trait Tickets: Send + Sync {
    /// The branches whose newest ticket from this session has not landed.
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>>;
}

pub struct NoTickets;

impl Tickets for NoTickets {
    fn unlanded(&self, _session: &str) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// The lander's queues on this machine: one directory per repository, its pending tickets at the top and its settled ones in areas below.
pub struct LandQueue {
    root: PathBuf,
}

impl LandQueue {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn here() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|path| path.is_absolute());
        let state = state.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self::at(state.join("north/land-queue"))
    }
}

impl Tickets for LandQueue {
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>> {
        let mut newest: BTreeMap<(PathBuf, String), (u128, Phase)> = BTreeMap::new();
        let queues = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        for queue in queues {
            let queue = queue?.path();
            if !queue.is_dir() {
                continue;
            }
            let store = land_ticket::recorded(&queue);
            for directory in land_ticket::directories(&queue) {
                for (path, queued, ticket) in tickets_in(&directory)? {
                    if ticket.get("session").and_then(Value::as_str) != Some(session) {
                        continue;
                    }
                    let branch = ticket.get("branch").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let phase = land_ticket::read(store.as_ref(), &queue, &path, ticket.get("verdict").and_then(Value::as_str));
                    let slot = newest.entry((queue.clone(), branch)).or_insert((queued, phase));
                    if queued >= slot.0 {
                        *slot = (queued, phase);
                    }
                }
            }
        }
        Ok(newest.into_iter().filter(|(_, (_, phase))| !matches!(phase, Phase::Landed | Phase::Withdrawn)).map(|((_, branch), _)| branch).collect())
    }
}

fn tickets_in(directory: &Path) -> NorthResult<Vec<(PathBuf, u128, Value)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut tickets = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let ticket: Value = serde_json::from_str(&fs::read_to_string(&path)?)?;
        let named = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok());
        let queued = ticket["queued_micros"].as_u64().map(u128::from).or(named).unwrap_or(u128::MAX);
        tickets.push((path, queued, ticket));
    }
    Ok(tickets)
}

