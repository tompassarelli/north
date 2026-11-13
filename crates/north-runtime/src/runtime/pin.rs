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

/// How long a due promotion may stay unserved before the settling host pages the coordinator: a stuck promotion is noticed before settle fails the landing at `landing_live_deadline_sec`.
const LIVE_PAGE: i64 = 5 * 60;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    Slot,
    FastPath,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    Landed { sha: String, at: i64 },
    Due { sha: String, reason: Reason, at: i64 },
    Promoted { sha: String, at: i64 },
    Walked { sha: String, host: String, at: i64 },
    RolledBack { sha: String, host: String, at: i64, seconds: i64, walker: bool },
    Built { sha: String, host: String, at: i64, seconds: i64, binary: String },
    /// The coordinator was paged about a due promotion not live `LIVE_PAGE` after it fell due.
    Paged { sha: String, at: i64 },
    /// One host of an ordered promotion: who ordered the commit and why, what that host's canary said before it went live, and whether it did.
    Ordered { sha: String, host: String, orderer: String, reason: String, canary: String, live: bool, at: i64 },
    /// How long a landing waited for its release runtime after the lander pushed: 0 s when the lander built the tree ahead while the lane queued.
    #[serde(rename = "artifact-ready")]
    ArtifactReady { sha: String, tree: String, seconds: i64, at: i64 },
    Canaried { sha: String, host: String, at: i64, seconds: i64 },
    Shipped { sha: String, host: String, at: i64, seconds: i64 },
    Slotted { slot: i64, outcome: Outcome, at: i64 },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "kebab-case")]
pub enum Outcome {
    Promoted,
    Refused { reason: String },
    Failed { probe: String },
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Promoted => write!(out, "promoted"),
            Self::Refused { reason } => write!(out, "refused: {reason}"),
            Self::Failed { probe } => write!(out, "failed: {probe}"),
        }
    }
}

impl Event {
    pub fn at(&self) -> i64 {
        match self {
            Self::Landed { at, .. } | Self::Due { at, .. } | Self::Promoted { at, .. } | Self::Walked { at, .. } | Self::RolledBack { at, .. } | Self::Built { at, .. } | Self::Paged { at, .. } | Self::Ordered { at, .. } | Self::ArtifactReady { at, .. } | Self::Canaried { at, .. } | Self::Shipped { at, .. } | Self::Slotted { at, .. } => *at,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Ahead {
    Current,
    Unchanged,
    Changed,
}

pub(super) fn ahead(pinned: Option<&str>, main: &str, changes_runtime: impl FnOnce(&str) -> bool) -> Ahead {
    match pinned {
        None => Ahead::Changed,
        Some(pinned) if pinned == main => Ahead::Current,
        Some(pinned) if changes_runtime(pinned) => Ahead::Changed,
        Some(_) => Ahead::Unchanged,
    }
}

/// How origin/main at `main` stands to the pin here: a head that changes a runtime input since the pin is journaled once as a landing, so the schedule and the runtime-age bar date its wait.
pub(super) fn observe(layout: &Layout, checkout: &Path, main: &str) -> Ahead {
    let pinned = layout.live_sha();
    let ahead = ahead(pinned.as_deref(), main, |pinned| !reuse::unchanged_by(checkout, pinned, main, runtime_input));
    if ahead == Ahead::Changed && !read(&layout.state).iter().any(|event| matches!(event, Event::Landed { sha, .. } if sha == main)) {
        note(layout, &Event::Landed { sha: main.to_owned(), at: now() });
    }
    ahead
}

/// The landings still waiting for a promotion, oldest first, with when each landed. A promotion of a landed sha makes live that landing and every one journaled before it, not those that landed after; a promotion of any other sha makes all of them live.
pub(super) fn waiting(events: &[Event]) -> Vec<(&str, i64)> {
    let mut landings: Vec<(&str, i64)> = Vec::new();
    for event in events {
        match event {
            Event::Landed { sha, at } => landings.push((sha, *at)),
            Event::Promoted { sha, .. } => match landings.iter().rposition(|(landed, _)| landed == sha) {
                Some(covered) => drop(landings.drain(..=covered)),
                None => landings.clear(),
            },
            _ => {}
        }
    }
    landings
}

pub(super) fn waiting_since(events: &[Event]) -> Option<i64> {
    waiting(events).first().map(|&(_, at)| at)
}

fn unserved(events: &[Event]) -> &[Event] {
    let served = events.iter().rposition(|event| matches!(event, Event::Promoted { .. })).map_or(0, |last| last + 1);
    &events[served..]
}

pub(super) fn owed(events: &[Event]) -> Vec<(&str, Reason, i64)> {
    unserved(events).iter().filter_map(|event| match event {
        Event::Due { sha, reason, at } => Some((sha.as_str(), *reason, *at)),
        _ => None,
    }).collect()
}

/// Pages the coordinator once per stuck promotion: when the oldest promotion that fell due (at its slot, or when a `Live-fix:` landing was seen) is not live `LIVE_PAGE` later. A landing waiting for its slot is on schedule and pages nothing, and due promotions that queue behind a stuck one join its page. `send` carries the text and says whether it was delivered, and only a delivered page is journaled, so a failed send is tried again by the next caller. A sha the live runtime is, or descends from (`contained(live, sha)`), is live whatever the journal holds.
pub(super) fn page(layout: &Layout, now: i64, host: &str, contained: &mut dyn FnMut(&str, &str) -> bool, send: &mut dyn FnMut(&str) -> bool) {
    let live = layout.live_sha();
    adopt_recorded_promotion(layout, live.as_deref());
    let events = read(&layout.state);
    let mut runs = |sha: &str| live.as_deref().is_some_and(|live| live == sha || contained(live, sha));
    let Some((sha, reason, due)) = owed(&events).into_iter().find(|&(sha, _, due)| now - due < LIVE_PAGE || !runs(sha)) else { return };
    let seconds = now - due;
    if seconds < LIVE_PAGE || unserved(&events).iter().any(|event| matches!(event, Event::Paged { .. })) {
        return;
    }
    let pinned = live.unwrap_or_else(|| "none".to_owned());
    let kind = match reason {
        Reason::Slot => "slot",
        Reason::FastPath => "Live-fix",
    };
    let delivered = send(&format!(
        "north runtime: the {kind} promotion of {} due at {} is not live 5 min later ({host} has waited {seconds}s for it; pinned runtime is {})",
        layout::short(sha),
        iso(due),
        layout::short(&pinned)
    ));
    if delivered {
        note(layout, &Event::Paged { sha: sha.to_owned(), at: now });
    }
}

/// The promotion lifecycle dates each runtime that went live here; the pin a host took before this journal existed is adopted once, so status and the bars date it.
pub(super) fn adopt_recorded_promotion(layout: &Layout, live: Option<&str>) {
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

/// The journal this host keeps when it holds a landing, else the copy of the settling host's that the records mirror holds.
pub(super) fn events_here_or_mirrored(state: &Path, reader: &str) -> Vec<Event> {
    let local = state.join(JOURNAL);
    let events = read_path(&local);
    match events.iter().any(|event| matches!(event, Event::Landed { .. })) {
        true => events,
        false => read_path(&crate::platform::records_mirror::read(reader).local(&local)),
    }
}

pub fn status(state: &Path, live: Option<&str>) -> String {
    let events = events_here_or_mirrored(state, "north runtime status");
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

pub(super) fn iso(at: i64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds(at).iso()
}

#[derive(Debug, PartialEq)]
pub struct Bars {
    /// None while the journal holds nothing before the window's end, so a fleet that never pinned shows no record rather than a pass.
    pub breaks_reaching_second_host: Option<usize>,
    pub rollback_seconds: Option<f64>,
    pub rollbacks: usize,
    /// Seconds from each promotion that fell due in the window to the promotion that served it, or to the window's end while it is still owed.
    pub due_to_live_seconds: Vec<f64>,
    /// How long the oldest landing still waiting for a promotion had waited at the window's end: 0 when none waits.
    pub runtime_age_hours: Option<f64>,
    pub canary_seconds: Vec<f64>,
    pub ship_seconds: Vec<f64>,
}

/// Each bar as its value and the sample it was read over, for the scoreboard.
impl Bars {
    pub fn breaks(&self) -> Option<(f64, usize)> {
        self.breaks_reaching_second_host.map(|breaks| (breaks as f64, 1))
    }

    pub fn rollback(&self) -> Option<(f64, usize)> {
        self.rollback_seconds.map(|seconds| (seconds, self.rollbacks))
    }

    pub fn age(&self) -> Option<(f64, usize)> {
        self.runtime_age_hours.map(|hours| (hours, 1))
    }
}

pub fn bars(events: &[Event], start: i64, end: i64) -> Bars {
    let inside = |at: i64| at >= start && at < end;
    let rollbacks: Vec<f64> = events.iter().filter_map(|event| match event {
        Event::RolledBack { at, seconds, .. } if inside(*at) => Some(*seconds as f64),
        _ => None,
    }).collect();
    let before: Vec<Event> = events.iter().filter(|event| event.at() < end).cloned().collect();
    let breaks = events.iter().filter(|event| matches!(event, Event::RolledBack { at, walker: false, .. } if inside(*at))).count();
    let age = waiting_since(&before).map_or(0.0, |since| (end - since) as f64 / 3600.0);
    Bars {
        breaks_reaching_second_host: (!before.is_empty()).then_some(breaks),
        rollback_seconds: rollbacks.iter().copied().reduce(f64::max),
        rollbacks: rollbacks.len(),
        due_to_live_seconds: due_to_live(&before, end).into_iter().filter(|(due, _)| inside(*due)).map(|(due, live)| (live - due) as f64).collect(),
        runtime_age_hours: (!before.is_empty()).then_some(age),
        canary_seconds: events.iter().filter_map(|event| match event {
            Event::Canaried { at, seconds, .. } if inside(*at) => Some(*seconds as f64),
            _ => None,
        }).collect(),
        ship_seconds: events.iter().filter_map(|event| match event {
            Event::Shipped { at, seconds, .. } if inside(*at) => Some(*seconds as f64),
            _ => None,
        }).collect(),
    }
}

/// Each promotion that fell due with when it went live: the first promotion journaled after it, else `end` while it is still owed.
fn due_to_live(events: &[Event], end: i64) -> Vec<(i64, i64)> {
    let mut owed = Vec::new();
    let mut timed = Vec::new();
    for event in events {
        match event {
            Event::Due { at, .. } => owed.push(*at),
            Event::Promoted { at, .. } => timed.extend(owed.drain(..).map(|due| (due, (*at).max(due)))),
            _ => {}
        }
    }
    timed.extend(owed.into_iter().map(|due| (due, end)));
    timed
}

pub(super) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests;
