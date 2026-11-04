use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub origin: String,
    pub root: PathBuf,
    pub host: String,
    pub branch: String,
    pub head: String,
    pub session: Option<String>,
    pub queued_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_commit_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub several_commits: Option<String>,
    pub red_fix: Option<RedFix>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_lane: bool,
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

pub fn queued_micros(path: &Path) -> u128 {
    path.file_name().and_then(|name| name.to_str()).and_then(|name| name.split('-').next()).and_then(|micros| micros.parse().ok()).unwrap_or(u128::MAX)
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
    tickets.sort_by_key(|(path, ticket)| (ticket.red_fix.is_none(), queued_micros(path), path.clone()));
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
    let mut newest: HashMap<(String, PathBuf, String), u128> = HashMap::new();
    for (path, ticket) in &tickets {
        let entry = newest.entry((ticket.host.clone(), ticket.root.clone(), ticket.branch.clone())).or_insert(0);
        *entry = (*entry).max(queued_micros(path));
    }
    let mut kept = Vec::new();
    for (path, ticket) in tickets {
        let lane = (ticket.host.clone(), ticket.root.clone(), ticket.branch.clone());
        if queued_micros(&path) < newest[&lane] {
            decide(&path, ticket, "replaced by a newer ticket for this lane".to_owned())?;
        } else {
            kept.push((path, ticket));
        }
    }
    Ok(kept)
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
