use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::land::{queue, replay};

pub const LANDINGS: usize = 3;
pub const SLOW_SECONDS: u64 = 120;
pub const STALL_SECONDS: u64 = 600;
const FILE: &str = "lander-watch";
const REPLACED: [&str; 2] = ["replaced", "withdrawn"];

#[derive(Debug, PartialEq, Eq)]
pub struct Watch {
    pub since: u64,
    pub candidate: PathBuf,
    pub previous: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Judgement {
    Armed,
    Settled,
    Breach(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decision {
    pub queued: u64,
    pub decided: Option<u64>,
    pub landed: bool,
}

pub fn armed(home: &Path) -> Option<Watch> {
    let text = fs::read_to_string(home.join(FILE)).ok()?;
    let mut lines = text.lines();
    Some(Watch { since: lines.next()?.trim().parse().ok()?, candidate: PathBuf::from(lines.next()?), previous: PathBuf::from(lines.next()?) })
}

pub fn arm(home: &Path, candidate: &Path, previous: &Path, since: u64) -> std::io::Result<()> {
    fs::create_dir_all(home)?;
    fs::write(home.join(FILE), format!("{since}\n{}\n{}\n", candidate.display(), previous.display()))
}

pub fn clear(home: &Path) {
    let _ = fs::remove_file(home.join(FILE));
}

pub fn judge(watch: &Watch, decisions: &[Decision], now: u64) -> Judgement {
    let own: Vec<Decision> = decisions.iter().copied().filter(|decision| decision.queued >= watch.since).collect();
    let landed: Vec<u64> = own.iter().filter(|decision| decision.landed).filter_map(|decision| decision.decided).collect();
    if let Some((start, end)) = own.iter().map(|decision| stretch(decision, watch.since, now)).find(|&(start, end)| end.saturating_sub(start) >= STALL_SECONDS && !landed.iter().any(|at| (start..=start + STALL_SECONDS).contains(at))) {
        return Judgement::Breach(format!("a ticket waited {} s with no landing in {} minutes", end - start, STALL_SECONDS / 60));
    }
    let latencies = head_to_main(&own, watch.since);
    match latencies.get(..LANDINGS) {
        Some(first) if first.iter().all(|&seconds| seconds > SLOW_SECONDS) => {
            let shown = first.iter().map(|seconds| format!("{seconds} s")).collect::<Vec<_>>().join(", ");
            Judgement::Breach(format!("the next {LANDINGS} landings each took more than {SLOW_SECONDS} s from queue head to main: {shown}"))
        }
        Some(_) => Judgement::Settled,
        None => Judgement::Armed,
    }
}

fn stretch(decision: &Decision, since: u64, now: u64) -> (u64, u64) {
    (decision.queued.max(since), decision.decided.unwrap_or(now))
}

fn head_to_main(own: &[Decision], since: u64) -> Vec<u64> {
    let mut decided: Vec<(u64, Decision)> = own.iter().filter_map(|decision| Some((decision.decided?, *decision))).collect();
    decided.sort_by_key(|(at, _)| *at);
    let mut previous = since;
    let mut latencies = Vec::new();
    for (at, decision) in decided {
        if decision.landed {
            latencies.push(at.saturating_sub(decision.queued.max(previous)));
        }
        previous = at;
    }
    latencies
}

pub fn decisions(home: &Path, since: u64) -> Vec<Decision> {
    let Ok(queues) = fs::read_dir(home.join("land-queue")) else { return Vec::new() };
    queues
        .filter_map(Result::ok)
        .flat_map(|entry| {
            let mut found = read_tickets(&entry.path().join("done"), since, true);
            found.extend(read_tickets(&entry.path(), since, false));
            found
        })
        .collect()
}

fn read_tickets(directory: &Path, since: u64, done: bool) -> Vec<Decision> {
    let Ok(entries) = fs::read_dir(directory) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .filter_map(|path| {
            let decided = done.then(|| modified_seconds(&path)).flatten();
            if done && decided.is_none_or(|at| at < since) {
                return None;
            }
            let ticket = queue::read(&path).ok()?;
            if ticket.verdict.as_deref().is_some_and(|verdict| REPLACED.iter().any(|kind| verdict.starts_with(kind))) {
                return None;
            }
            let micros = queue::queued_micros(&path);
            (micros != u128::MAX).then(|| Decision { queued: u64::try_from(micros / 1_000_000).unwrap_or(u64::MAX), decided, landed: ticket.verdict.as_deref().is_some_and(|verdict| replay::landed_tip(verdict).is_some()) })
        })
        .collect()
}

fn modified_seconds(path: &Path) -> Option<u64> {
    fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs())
}
