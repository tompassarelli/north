use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::reservation::{self, Decided};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedFix {
    Issue(u64),
    Marker,
}

impl Serialize for RedFix {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            RedFix::Issue(number) => serializer.serialize_u64(*number),
            RedFix::Marker => serializer.serialize_str("marker"),
        }
    }
}

impl<'de> Deserialize<'de> for RedFix {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Number(number) => number.as_u64().map(RedFix::Issue).ok_or_else(|| serde::de::Error::custom("red_fix issue must be a positive integer")),
            serde_json::Value::String(text) if text == "marker" => Ok(RedFix::Marker),
            other => Err(serde::de::Error::custom(format!("red_fix must be an issue number or \"marker\", not {other}"))),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub origin: String,
    pub root: PathBuf,
    pub host: String,
    pub branch: String,
    pub head: String,
    pub session: Option<String>,
    pub queued_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_micros: Option<u128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_commit_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub several_commits: Option<String>,
    pub red_fix: Option<RedFix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_lane: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
}

impl Ticket {
    pub fn goes_first(&self) -> bool {
        self.red_fix.is_some() || self.urgent.is_some()
    }
}

pub use crate::platform::land_queue::{directory, key};

pub const OUTBOX: &str = "outbox";

pub fn slug(branch: &str) -> String {
    branch.chars().map(|character| if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') { character } else { '-' }).collect()
}

pub fn file_name(queued_micros: u128, branch: &str) -> String {
    format!("{queued_micros}-{}.json", slug(branch))
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| io::Error::other(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.{}.tmp", path.file_name().unwrap_or_default().to_string_lossy(), std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}

pub fn write(path: &Path, ticket: &Ticket) -> io::Result<()> {
    write_atomic(path, &(serde_json::to_string_pretty(ticket).map_err(io::Error::other)? + "\n"))
}

pub fn read(path: &Path) -> io::Result<Ticket> {
    serde_json::from_str(&fs::read_to_string(path)?).map_err(|error| io::Error::other(format!("{}: {error}", path.display())))
}

fn named_micros(path: &Path) -> Option<u128> {
    path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok())
}

fn recorded_micros(path: &Path) -> Option<u128> {
    #[derive(Deserialize)]
    struct Recorded {
        queued_micros: Option<u128>,
    }
    serde_json::from_str::<Recorded>(&fs::read_to_string(path).ok()?).ok()?.queued_micros
}

/// When enqueue received the ticket, as it recorded inside the ticket; only a ticket written before that field existed falls back to the file-name id, so renaming a ticket never moves it in the queue.
pub fn queued_micros(path: &Path) -> u128 {
    recorded_micros(path).or_else(|| named_micros(path)).unwrap_or(u128::MAX)
}

pub type Lane = (String, PathBuf, String);

pub fn lane(ticket: &Ticket) -> Lane {
    (ticket.host.clone(), ticket.root.clone(), ticket.branch.clone())
}

fn decided_secs(path: &Path) -> u64 {
    fs::metadata(path).and_then(|metadata| metadata.modified()).ok().and_then(|modified| modified.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs())
}

pub fn positions(directory: &Path, tickets: &[(PathBuf, Ticket)], now_secs: u64) -> HashMap<PathBuf, u128> {
    let lanes: HashSet<Lane> = tickets.iter().map(|(_, ticket)| lane(ticket)).collect();
    let suffixes: HashSet<String> = tickets.iter().map(|(_, ticket)| format!("-{}.json", slug(&ticket.branch))).collect();
    let mut decided: HashMap<Lane, Vec<(u128, u64, String)>> = HashMap::new();
    for entry in fs::read_dir(directory.join("done")).into_iter().flatten().filter_map(Result::ok) {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !suffixes.iter().any(|suffix| name.ends_with(suffix.as_str())) {
            continue;
        }
        let Ok(ticket) = read(&path) else { continue };
        let key = lane(&ticket);
        if lanes.contains(&key) {
            decided.entry(key).or_default().push((queued_micros(&path), decided_secs(&path), ticket.verdict.unwrap_or_default()));
        }
    }
    tickets
        .iter()
        .map(|(path, ticket)| {
            let queued = queued_micros(path);
            let mut earlier: Vec<_> = decided.get(&lane(ticket)).into_iter().flatten().filter(|(micros, _, _)| *micros < queued).collect();
            earlier.sort_by_key(|(micros, _, _)| std::cmp::Reverse(*micros));
            let newest_first: Vec<Decided> = earlier.into_iter().map(|(micros, secs, verdict)| Decided { queued: *micros, decided_secs: *secs, verdict }).collect();
            (path.clone(), reservation::position(queued, &newest_first, now_secs))
        })
        .collect()
}

pub fn pending(directory: &Path) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut tickets = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "json") && path.is_file() {
            let ticket = read(&path)?;
            tickets.push((path, ticket));
        }
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let positions = positions(directory, &tickets, now);
    tickets.sort_by_key(|(path, ticket)| (!ticket.goes_first(), positions[path], path.clone()));
    Ok(tickets)
}

pub fn decide(path: &Path, ticket: Ticket, verdict: String) -> io::Result<PathBuf> {
    settle(path, ticket, verdict, "done")
}

pub fn settle(path: &Path, mut ticket: Ticket, verdict: String, area: &str) -> io::Result<PathBuf> {
    let directory = path.parent().ok_or_else(|| io::Error::other(format!("{} has no parent", path.display())))?;
    let done = directory.join(area).join(path.file_name().ok_or_else(|| io::Error::other(format!("{} has no file name", path.display())))?);
    ticket.verdict = Some(verdict);
    write(&done, &ticket)?;
    fs::remove_file(path)?;
    Ok(done)
}

pub fn collect(directory: &Path) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let tickets = pending(directory)?;
    let mut newest: HashMap<Lane, u128> = HashMap::new();
    for (path, ticket) in &tickets {
        let entry = newest.entry(lane(ticket)).or_insert(0);
        *entry = (*entry).max(queued_micros(path));
    }
    let mut kept = Vec::new();
    for (path, ticket) in tickets {
        if queued_micros(&path) < newest[&lane(&ticket)] {
            decide(&path, ticket, reservation::REPLACED.to_owned())?;
        } else {
            kept.push((path, ticket));
        }
    }
    Ok(kept)
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
