use super::layout::{self, Layout};
use super::lifecycle::{self, Promoting, Promotion};
use crate::platform::process;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const JOURNAL: &str = "runtime-pins.jsonl";
pub const PER_DAY: usize = 6;
const DAY_S: i64 = 86_400;
const SPACING_S: i64 = DAY_S / PER_DAY as i64;
const RED_FIX: &str = "Main-red:";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    First,
    Cadence,
    RedMainFix,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Event {
    Promoted { sha: String, at: i64, reason: Reason },
    Walked { sha: String, host: String, at: i64 },
    RolledBack { sha: String, host: String, at: i64, seconds: i64, walker: bool },
    Waiting { sha: String, at: i64 },
}

impl Event {
    pub fn at(&self) -> i64 {
        match self {
            Self::Promoted { at, .. } | Self::Walked { at, .. } | Self::RolledBack { at, .. } | Self::Waiting { at, .. } => *at,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Due {
    Stay { next_at: i64 },
    Promote(Reason),
}

pub(super) fn due(pinned: Option<&str>, main: &str, promoted: &[i64], now: i64, red_fix: bool) -> Due {
    let Some(pinned) = pinned else { return Due::Promote(Reason::First) };
    let mut recent: Vec<i64> = promoted.iter().copied().filter(|at| *at > now - DAY_S).collect();
    recent.sort_unstable();
    let spaced = recent.last().map_or(0, |last| last + SPACING_S);
    let capped = recent.len().checked_sub(PER_DAY).map_or(0, |oldest| recent[oldest] + DAY_S);
    let next_at = spaced.max(capped);
    match (pinned == main, red_fix, now >= next_at) {
        (true, _, _) => Due::Stay { next_at },
        (false, true, _) => Due::Promote(Reason::RedMainFix),
        (false, false, true) => Due::Promote(Reason::Cadence),
        (false, false, false) => Due::Stay { next_at },
    }
}

pub(super) fn target(layout: &Layout, checkout: &Path, main: &str) -> (String, Option<Reason>) {
    let pinned = layout.live_sha();
    let mut events = read(&layout.state);
    adopt_recorded_promotion(layout, pinned.as_deref(), &mut events);
    let promoted: Vec<i64> = events.iter().filter_map(|event| match event {
        Event::Promoted { at, .. } => Some(*at),
        _ => None,
    }).collect();
    let red_fix = pinned.as_deref().is_some_and(|pinned| fixes_red_main(checkout, pinned, main));
    match (due(pinned.as_deref(), main, &promoted, now(), red_fix), pinned) {
        (Due::Promote(reason), _) => (main.to_owned(), Some(reason)),
        (Due::Stay { next_at }, Some(pinned)) => {
            if pinned != main {
                println!("north runtime pinned: {} stays live; {} waits for the next promotion at unix {next_at} or a {RED_FIX} fix", layout::short(&pinned), layout::short(main));
                if waiting_since(&events, i64::MAX).is_none() {
                    note(layout, &Event::Waiting { sha: main.to_owned(), at: now() });
                }
            }
            (pinned, None)
        }
        (Due::Stay { .. }, None) => (main.to_owned(), Some(Reason::First)),
    }
}

/// When main first moved past the pinned runtime and has waited since: the first `Waiting` after the last promotion, in journal order.
fn waiting_since(events: &[Event], end: i64) -> Option<i64> {
    events.iter().filter(|event| event.at() < end).fold(None, |since, event| match event {
        Event::Promoted { .. } => None,
        Event::Waiting { at, .. } => since.or(Some(*at)),
        _ => since,
    })
}

/// The promotion lifecycle dates each runtime that went live here; the pin a host took before this journal existed is adopted once, so status and the bars date it.
fn adopt_recorded_promotion(layout: &Layout, live: Option<&str>, events: &mut Vec<Event>) {
    let Some(live) = live else { return };
    if events.iter().any(|event| matches!(event, Event::Promoted { sha, .. } if sha == live)) {
        return;
    }
    let Some(at) = recorded_promotion(&layout.state, live) else { return };
    let event = Event::Promoted { sha: live.to_owned(), at, reason: Reason::First };
    note(layout, &event);
    events.push(event);
}

fn recorded_promotion(state: &Path, live: &str) -> Option<i64> {
    let layout = Layout { state: state.to_owned() };
    lifecycle::record_of::<Promoting>(&layout, live).ok().flatten().filter(|record| record.state == Promotion::Live).map(|record| record.since.unix_seconds())
}

fn fixes_red_main(checkout: &Path, pinned: &str, main: &str) -> bool {
    let range = format!("{pinned}..{main}");
    process::run("git", Some(checkout), &["log", "--format=%B", &range])
        .ok()
        .filter(|output| output.status.success())
        .is_some_and(|output| String::from_utf8_lossy(&output.stdout).lines().any(|line| line.strip_prefix(RED_FIX).is_some_and(|rest| rest.starts_with(char::is_whitespace) && !rest.trim().is_empty())))
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

pub(super) fn promoted(layout: &Layout, sha: &str, reason: Reason) {
    note(layout, &Event::Promoted { sha: sha.to_owned(), at: now(), reason });
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
    let last = events.iter().rev().find_map(|event| match event {
        Event::Promoted { sha, at, reason } if live.is_none_or(|live| live == sha) => Some((sha.as_str(), *at, *reason)),
        _ => None,
    });
    let today = events.iter().filter(|event| matches!(event, Event::Promoted { at, .. } if *at > now() - DAY_S)).count();
    let walked = live.is_some_and(|live| events.iter().any(|event| matches!(event, Event::Walked { sha, .. } if sha == live)));
    let waiting = waiting_since(&events, i64::MAX).map_or_else(|| "main waiting since: none\n".to_owned(), |since| format!("main waiting since: {} (unix {since})\n", iso(since)));
    match (live, last) {
        (None, _) => "pinned: none (no live runtime on this host)\n".to_owned(),
        (Some(live), Some((_, at, reason))) => format!("pinned: {live}\npromoted at: {} (unix {at}, {reason:?})\nspine walked: {}\npromotions in 24 h: {today} of at most {PER_DAY}\n{waiting}", iso(at), if walked { "yes" } else { "no" }),
        (Some(live), None) => match recorded_promotion(state, live) {
            Some(at) => format!("pinned: {live}\npromoted at: {} (unix {at}, recorded on this host)\nspine walked: {}\npromotions in 24 h: {today} of at most {PER_DAY}\n{waiting}", iso(at), if walked { "yes" } else { "no" }),
            None => format!("pinned: {live}\npromoted at: unrecorded on this host (the settling host records promotions)\npromotions in 24 h: {today} of at most {PER_DAY}\n{waiting}"),
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
    /// How long origin/main has waited for the pinned runtime at the window's end: 0 when the pin is main.
    pub runtime_age_hours: Option<f64>,
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
    let recorded = events.iter().any(|event| event.at() < end);
    let breaks = events.iter().filter(|event| matches!(event, Event::RolledBack { at, walker: false, .. } if inside(*at))).count();
    let age = waiting_since(events, end).map_or(0.0, |since| (end - since) as f64 / 3600.0);
    Bars {
        breaks_reaching_second_host: recorded.then_some(breaks),
        rollback_seconds: rollbacks.iter().copied().reduce(f64::max),
        rollbacks: rollbacks.len(),
        runtime_age_hours: recorded.then_some(age),
    }
}

pub(super) fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests;
