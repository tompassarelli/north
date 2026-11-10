use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::land_ticket::{self, Phase};

use super::reservation::{self, Decided};
use north_base::land_ticket::Records;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deferral: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_lane: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgent_at: Option<u128>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lander_fix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alone_green: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alone_base: Option<String>,
}

impl Ticket {
    pub fn passed_alone(&self) -> bool {
        self.alone_green.as_deref() == Some(self.head.as_str())
    }

    pub fn passed_alone_on(&self) -> Option<&str> {
        self.alone_base.as_deref().filter(|_| self.passed_alone())
    }

    pub fn goes_first(&self) -> bool {
        self.red_fix.is_some() || self.urgent.is_some() || self.lander_fix.is_some()
    }
}

pub use crate::platform::land_queue::{confined, directory, key, slug};

pub const QUARANTINE: &str = "quarantine";
pub const OUTBOX: &str = "outbox";
pub const LANDER_LEDGER: &str = "lander-post-land.tsv";

pub fn rfc3339(now: time::OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}
pub const WITHDRAWN: &str = "withdrawn";

pub fn file_name(queued_micros: u128, branch: &str) -> String {
    format!("{queued_micros}-{}.json", slug(branch))
}

pub fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
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

pub fn defer(path: &Path, reason: &str) -> io::Result<()> {
    let mut ticket = read(path)?;
    ticket.deferral = Some(reason.trim().to_owned());
    write(path, &ticket)
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

pub fn rank(ticket: &Ticket, position: u128, path: &Path) -> (bool, bool, bool, std::cmp::Reverse<u128>, u128, PathBuf) {
    (ticket.lander_fix.is_none(), ticket.red_fix.is_none(), !ticket.goes_first(), std::cmp::Reverse(ticket.urgent_at.unwrap_or(0)), position, path.to_path_buf())
}

pub fn queue_position(directory: &Path, path: &Path) -> io::Result<Option<(usize, usize)>> {
    let tickets = pending(directory)?;
    let total = tickets.len();
    Ok(tickets.iter().position(|(candidate, _)| candidate == path).map(|at| (at + 1, total)))
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
    let store = std::cell::OnceCell::new();
    let mut decided: HashMap<Lane, Vec<(u128, u64, Phase)>> = HashMap::new();
    for entry in fs::read_dir(directory.join("done")).into_iter().flatten().filter_map(Result::ok) {
        let path = entry.path();
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if !suffixes.iter().any(|suffix| name.ends_with(suffix.as_str())) {
            continue;
        }
        let Ok(ticket) = read(&path) else { continue };
        let phase = land_ticket::read(store.get_or_init(|| land_ticket::recorded(directory)).as_ref(), directory, &path, ticket.verdict.as_deref());
        let key = lane(&ticket);
        if lanes.contains(&key) {
            decided.entry(key).or_default().push((queued_micros(&path), decided_secs(&path), phase));
        }
    }
    tickets
        .iter()
        .map(|(path, ticket)| {
            let queued = queued_micros(path);
            let mut earlier: Vec<_> = decided.get(&lane(ticket)).into_iter().flatten().filter(|(micros, _, _)| *micros < queued).collect();
            earlier.sort_by_key(|(micros, _, _)| std::cmp::Reverse(*micros));
            let newest_first: Vec<Decided> = earlier.into_iter().map(|(micros, secs, phase)| Decided { queued: *micros, decided_secs: *secs, phase: *phase }).collect();
            (path.clone(), reservation::position(queued, &newest_first, now_secs))
        })
        .collect()
}

fn read_unless_moved(path: &Path) -> io::Result<Option<Ticket>> {
    match read(path) {
        Ok(ticket) => Ok(Some(ticket)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn listed(directory: &Path) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut tickets = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "json")
            && path.is_file()
            && let Some(ticket) = read_unless_moved(&path)?
        {
            tickets.push((path, ticket));
        }
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let positions = positions(directory, &tickets, now);
    tickets.sort_by_key(|(path, ticket)| rank(ticket, positions[path], path));
    Ok(tickets)
}

/// The tickets waiting in `directory`, first to be checked first: every ticket file there but one whose record says it
/// was decided and whose queue copy a lander has yet to clear.
pub fn pending(directory: &Path) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let store = land_ticket::recorded(directory);
    let mut tickets = listed(directory)?;
    tickets.retain(|(path, _)| !store.as_ref().and_then(|store| land_ticket::phase(store, directory, path)).is_some_and(Phase::is_decided));
    Ok(tickets)
}

pub fn decide(path: &Path, ticket: Ticket, verdict: String) -> io::Result<PathBuf> {
    let phase = Phase::decided(&verdict);
    settle(path, ticket, verdict, phase)
}

fn queue_of(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(|| io::Error::other(format!("{} has no queue", path.display())))
}

/// Writes `ticket` into the area of `queue` its `phase` keeps it in and removes the copy at `path`.
fn place(queue: &Path, path: &Path, ticket: &Ticket, phase: Phase) -> io::Result<PathBuf> {
    let to = phase.area().map_or_else(|| queue.to_path_buf(), |area| queue.join(area)).join(path.file_name().unwrap_or_default());
    write(&to, ticket)?;
    if to != path {
        match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
    }
    Ok(to)
}

/// The withdrawal asked for while a lander checked the ticket at `path`: its copy in withdrawn/, which the lander's next
/// move of the ticket honours unless its check landed it.
fn withdrawal(queue: &Path, path: &Path) -> Option<Ticket> {
    read(&queue.join(WITHDRAWN).join(path.file_name()?)).ok()
}

/// Decides the ticket at `path`: its record moves to `phase` with the verdict as the cause, then its file moves to the
/// phase's area. A ticket still in a lane host's outbox has no record yet: the lander records it when it takes it.
pub fn settle(path: &Path, mut ticket: Ticket, verdict: String, phase: Phase) -> io::Result<PathBuf> {
    let (verdict, phase) = match withdrawal(queue_of(path)?, path) {
        Some(asked) if !matches!(phase, Phase::Landed | Phase::Withdrawn) => (asked.verdict.unwrap_or(verdict), Phase::Withdrawn),
        _ => (verdict, phase),
    };
    if queue_of(path)?.file_name() != Some(std::ffi::OsStr::new(OUTBOX)) {
        Records::of(path)?.moved(path, phase, &verdict)?;
    }
    ticket.verdict = Some(verdict);
    place(queue_of(path)?, path, &ticket, phase)
}

/// Moves a failed, conflicted or quarantined ticket at `decided` back into `queue` for another check.
pub fn requeue(queue: &Path, decided: &Path, cause: &str) -> io::Result<PathBuf> {
    let mut ticket = read(decided)?;
    let mut records = Records::open(queue)?;
    if records.phase(decided).is_none() {
        records.adopted(decided, ticket.verdict.as_deref())?;
    }
    records.moved(decided, Phase::Queued, cause)?;
    ticket.verdict = None;
    ticket.deferral = None;
    ticket.decided_on = None;
    place(queue, decided, &ticket, Phase::Queued)
}

/// At a lander's start: records every ticket file of `queue` that has no record in the phase adoption gives it, and
/// puts back every ticket a lander that is gone was checking. Returns how many it recorded.
pub fn adopt(queue: &Path) -> io::Result<usize> {
    let mut records = Records::open(queue)?;
    let mut adopted = 0;
    for directory in land_ticket::directories(queue) {
        for path in fs::read_dir(&directory).into_iter().flatten().flatten().map(|entry| entry.path()) {
            if path.extension().is_some_and(|extension| extension == "json") && records.phase(&path).is_none() {
                let Ok(ticket) = read(&path) else { continue };
                records.adopted(&path, ticket.verdict.as_deref())?;
                adopted += 1;
            }
        }
    }
    records.put_back()?;
    Ok(adopted)
}

/// The tickets of `queue` a lander may take. A ticket with no record is adopted first, and a decided ticket's queue copy
/// goes: its area keeps the ticket, and when the area has none (a settle its lander did not finish), the ticket moves
/// there with its record's cause as its verdict.
fn takeable(queue: &Path, tickets: Vec<(PathBuf, Ticket)>) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let mut records = Records::open(queue)?;
    let mut kept = Vec::with_capacity(tickets.len());
    for (path, mut ticket) in tickets {
        let phase = match records.phase(&path) {
            Some(phase) => phase,
            None => records.adopted(&path, ticket.verdict.as_deref())?,
        };
        if let Some(asked) = withdrawal(queue, &path).filter(|_| phase == Phase::Queued) {
            records.moved(&path, Phase::Withdrawn, asked.verdict.as_deref().unwrap_or("withdrawn during its check"))?;
            fs::remove_file(&path)?;
            continue;
        }
        if !phase.is_decided() {
            kept.push((path, ticket));
        } else if phase.area().is_some_and(|area| queue.join(area).join(path.file_name().unwrap_or_default()).is_file()) {
            fs::remove_file(&path)?;
        } else {
            ticket.verdict = records.cause(&path)?;
            place(queue, &path, &ticket, phase)?;
        }
    }
    Ok(kept)
}

pub fn collect(directory: &Path) -> io::Result<Vec<(PathBuf, Ticket)>> {
    let tickets = takeable(directory, listed(directory)?)?;
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

#[cfg(test)]
#[path = "ticket_lifecycle_tests.rs"]
mod ticket_lifecycle;
