use super::layout::{self, Layout};
use super::lifecycle::{self, Promoting, Promotion};
use super::reuse;
use north_base::build_inputs::runtime_input;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const JOURNAL: &str = "runtime-pins.jsonl";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    Landed { sha: String, at: i64 },
    Promoted { sha: String, at: i64 },
    Walked { sha: String, host: String, at: i64 },
    RolledBack { sha: String, host: String, at: i64, seconds: i64, walker: bool },
}

impl Event {
    pub fn at(&self) -> i64 {
        match self {
            Self::Landed { at, .. } | Self::Promoted { at, .. } | Self::Walked { at, .. } | Self::RolledBack { at, .. } => *at,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Due {
    Current,
    Unchanged,
    Promote,
}

pub(super) fn due(pinned: Option<&str>, main: &str, changes_runtime: impl FnOnce(&str) -> bool) -> Due {
    match pinned {
        None => Due::Promote,
        Some(pinned) if pinned == main => Due::Current,
        Some(pinned) if changes_runtime(pinned) => Due::Promote,
        Some(_) => Due::Unchanged,
    }
}

/// Whether origin/main at `main` needs a promotion here: a landing that changes a runtime input since the pin is journaled once, so the scoreboard times it to live.
pub(super) fn observe(layout: &Layout, checkout: &Path, main: &str) -> Due {
    let pinned = layout.live_sha();
    let due = due(pinned.as_deref(), main, |pinned| !reuse::unchanged_by(checkout, pinned, main, runtime_input));
    if due == Due::Promote && !read(&layout.state).iter().any(|event| matches!(event, Event::Landed { sha, .. } if sha == main)) {
        note(layout, &Event::Landed { sha: main.to_owned(), at: now() });
    }
    due
}

/// The runtime this host should run: origin/main when a commit since the pin changes a runtime input, else the pin it runs.
pub(super) fn target(layout: &Layout, checkout: &Path, main: &str) -> String {
    let pinned = layout.live_sha();
    adopt_recorded_promotion(layout, pinned.as_deref());
    match (observe(layout, checkout, main), pinned) {
        (Due::Unchanged, Some(pinned)) => {
            println!("north runtime pinned: {} stays live; {} changes no runtime input since it", layout::short(&pinned), layout::short(main));
            pinned
        }
        _ => main.to_owned(),
    }
}

/// When the oldest landing still waiting for a promotion landed: one journaled after the last promotion, in journal order.
fn waiting_since(events: &[Event]) -> Option<i64> {
    events.iter().fold(None, |since, event| match event {
        Event::Promoted { .. } => None,
        Event::Landed { at, .. } => since.or(Some(*at)),
        _ => since,
    })
}

/// The promotion lifecycle dates each runtime that went live here; the pin a host took before this journal existed is adopted once, so status and the bars date it.
fn adopt_recorded_promotion(layout: &Layout, live: Option<&str>) {
    let Some(live) = live else { return };
    if read(&layout.state).iter().any(|event| matches!(event, Event::Promoted { sha, .. } if sha == live)) {
        return;
    }
    if let Some(at) = recorded_promotion(&layout.state, live) {
        note(layout, &Event::Promoted { sha: live.to_owned(), at });
    }
}

fn recorded_promotion(state: &Path, live: &str) -> Option<i64> {
    let layout = Layout { state: state.to_owned() };
    lifecycle::record_of::<Promoting>(&layout, live).ok().flatten().filter(|record| record.state == Promotion::Live).map(|record| record.since.unix_seconds())
}

pub fn read(state: &Path) -> Vec<Event> {
    read_path(&state.join(JOURNAL))
}

fn read_path(path: &Path) -> Vec<Event> {
    fs::read_to_string(path).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

pub(super) fn note(layout: &Layout, event: &Event) {
    let Ok(line) = serde_json::to_string(event) else { return };
    let written = fs::OpenOptions::new().create(true).append(true).open(layout.state.join(JOURNAL)).and_then(|mut file| writeln!(file, "{line}"));
    if let Err(error) = written {
        eprintln!("north runtime pin: could not record {line}: {error}");
    }
}

pub(super) fn promoted(layout: &Layout, sha: &str) {
    note(layout, &Event::Promoted { sha: sha.to_owned(), at: now() });
}

pub fn status(state: &Path, live: Option<&str>) -> String {
    let local = state.join(JOURNAL);
    let events = match local.is_file() {
        true => read_path(&local),
        false => {
            let records = crate::platform::records_mirror::read("north runtime status");
            read_path(&records.local(&local))
        }
    };
    let promoted = live.and_then(|live| events.iter().rev().find_map(|event| match event {
        Event::Promoted { sha, at } if sha == live => Some(*at),
        _ => None,
    }));
    let walked = live.is_some_and(|live| events.iter().any(|event| matches!(event, Event::Walked { sha, .. } if sha == live)));
    let waiting = waiting_since(&events).map_or_else(|| "landings waiting since: none\n".to_owned(), |since| format!("landings waiting since: {} (unix {since})\n", iso(since)));
    let walked = format!("spine walked: {}\n", if walked { "yes" } else { "no" });
    match (live, promoted) {
        (None, _) => "pinned: none (no live runtime on this host)\n".to_owned(),
        (Some(live), Some(at)) => format!("pinned: {live}\npromoted at: {} (unix {at})\n{walked}{waiting}", iso(at)),
        (Some(live), None) => match recorded_promotion(state, live) {
            Some(at) => format!("pinned: {live}\npromoted at: {} (unix {at}, recorded on this host)\n{walked}{waiting}", iso(at)),
            None => format!("pinned: {live}\npromoted at: unrecorded on this host (the settling host records promotions)\n{waiting}"),
        },
    }
}

fn iso(at: i64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds(at).iso()
}

#[derive(Debug, PartialEq)]
pub struct Bars {
    /// None while the journal holds nothing before the window's end, so a fleet that never pinned shows no record rather than a pass.
    pub breaks_reaching_second_host: Option<usize>,
    pub rollback_seconds: Option<f64>,
    pub rollbacks: usize,
    /// Seconds from each landing in the window that changed a runtime input to the promotion that made it live, or to the window's end while it still waits.
    pub landing_to_live_seconds: Vec<f64>,
}

/// Each bar as its value and the sample it was read over, for the scoreboard.
impl Bars {
    pub fn breaks(&self) -> Option<(f64, usize)> {
        self.breaks_reaching_second_host.map(|breaks| (breaks as f64, 1))
    }

    pub fn rollback(&self) -> Option<(f64, usize)> {
        self.rollback_seconds.map(|seconds| (seconds, self.rollbacks))
    }
}

pub fn bars(events: &[Event], start: i64, end: i64) -> Bars {
    let inside = |at: i64| at >= start && at < end;
    let rollbacks: Vec<f64> = events.iter().filter_map(|event| match event {
        Event::RolledBack { at, seconds, .. } if inside(*at) => Some(*seconds as f64),
        _ => None,
    }).collect();
    let recorded = events.iter().any(|event| event.at() < end);
    let breaks = events.iter().filter(|event| matches!(event, Event::RolledBack { at, walker: false, .. } if inside(*at))).count();
    Bars {
        breaks_reaching_second_host: recorded.then_some(breaks),
        rollback_seconds: rollbacks.iter().copied().reduce(f64::max),
        rollbacks: rollbacks.len(),
        landing_to_live_seconds: landing_to_live(events, end).into_iter().filter(|(landed, _)| inside(*landed)).map(|(landed, live)| (live - landed) as f64).collect(),
    }
}

/// Each journaled landing with when it went live: the first promotion journaled after it, else `end` while it still waits.
fn landing_to_live(events: &[Event], end: i64) -> Vec<(i64, i64)> {
    let mut waiting = Vec::new();
    let mut timed = Vec::new();
    for event in events.iter().filter(|event| event.at() < end) {
        match event {
            Event::Landed { at, .. } => waiting.push(*at),
            Event::Promoted { at, .. } => timed.extend(waiting.drain(..).map(|landed| (landed, (*at).max(landed)))),
            _ => {}
        }
    }
    timed.extend(waiting.into_iter().map(|landed| (landed, end)));
    timed
}

pub(super) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests;
