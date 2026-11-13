use super::layout::{self, Layout};
use super::pin::{self, Ahead, Event, Outcome, Reason};
use super::probe;
use crate::platform::process;
use north_base::build_inputs::{live_fix, promotion_slot};
use north_base::platform::harness::{DEFAULTS, document};
use std::fs;
use std::path::Path;

const DAY: i64 = 86_400;
const BUILT_FOR: &str = "runtime-slot-built";
const PROMOTION: &str = "runtime_promotion";
const SLOT: &str = "slot";

#[derive(Clone, Copy, Debug)]
pub(super) struct Schedule {
    pub(super) spacing: i64,
    pub(super) lead: i64,
    pub(super) on_landing: bool,
}

impl Schedule {
    fn declared() -> Self {
        Self { spacing: probe::default_number("runtime_promotion_slot_sec") as i64, lead: probe::default_number("runtime_slot_build_lead_sec") as i64, on_landing: document(DEFAULTS)[PROMOTION]["value"].as_str() != Some(SLOT) }
    }

    fn slot(self, now: i64) -> i64 {
        promotion_slot(now, self.spacing)
    }

    fn taken(self, events: &[Event], slot: i64) -> bool {
        events.iter().any(|event| match event {
            Event::Due { reason: Reason::Slot, at, .. } => *at >= slot,
            Event::Due { reason: Reason::FastPath, at, .. } => (slot - self.spacing..slot).contains(at),
            _ => false,
        })
    }

    fn next_free(self, events: &[Event], now: i64) -> i64 {
        let mut slot = self.slot(now) + self.spacing;
        while self.taken(events, slot) {
            slot += self.spacing;
        }
        slot
    }
}

pub(super) fn promotes_on_landing() -> bool {
    Schedule::declared().on_landing
}

pub(super) fn promotion_slot_sec() -> i64 {
    Schedule::declared().spacing
}

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Open(Reason, i64),
    Promote(Reason),
    Build(i64),
    Wait(i64),
}

fn call(schedule: Schedule, events: &[Event], now: i64, fix: Option<&str>) -> Call {
    if fix.is_some_and(|fix| !events.iter().any(|event| matches!(event, Event::Due { sha, reason: Reason::FastPath, .. } if sha == fix))) {
        return Call::Open(Reason::FastPath, now);
    }
    let owed = pin::owed(events);
    if !owed.is_empty() {
        let fast = owed.iter().any(|&(_, reason, _)| reason == Reason::FastPath);
        return Call::Promote(if fast { Reason::FastPath } else { Reason::Slot });
    }
    let slot = schedule.slot(now);
    let next = slot + schedule.spacing;
    match pin::waiting_since(events) {
        Some(landed) if landed <= slot && !schedule.taken(events, slot) => Call::Open(Reason::Slot, slot),
        Some(_) if now >= next - schedule.lead && !schedule.taken(events, next) => Call::Build(next),
        _ => Call::Wait(schedule.next_free(events, now)),
    }
}

pub(super) struct Heads<'a> {
    pub(super) pinned: &'a str,
    pub(super) main: &'a str,
    pub(super) fix: Option<&'a str>,
}

pub(super) fn target(layout: &Layout, checkout: &Path, main: &str, ready: &dyn Fn(&str) -> bool, build: &mut dyn FnMut(&str)) -> String {
    let pinned = layout.live_sha();
    pin::adopt_recorded_promotion(layout, pinned.as_deref());
    match (pin::observe(layout, checkout, main), pinned) {
        (Ahead::Unchanged, Some(pinned)) => {
            println!("north runtime pinned: {} stays live; {} changes no runtime input since it", layout::short(&pinned), layout::short(main));
            pinned
        }
        (Ahead::Changed, Some(pinned)) => {
            let fix = fix_since(checkout, &pinned, main);
            scheduled(layout, Schedule::declared(), pin::now(), &Heads { pinned: &pinned, main, fix: fix.as_deref() }, ready, build)
        }
        _ => main.to_owned(),
    }
}

fn scheduled(layout: &Layout, schedule: Schedule, now: i64, heads: &Heads, ready: &dyn Fn(&str) -> bool, build: &mut dyn FnMut(&str)) -> String {
    let events = pin::read(&layout.state);
    let waits = |slot: i64| {
        println!("north runtime pinned: {} stays live; {} waits for the {} slot or a Live-fix landing", layout::short(heads.pinned), layout::short(heads.main), clock(slot));
        heads.pinned.to_owned()
    };
    let fix = heads.fix.or(schedule.on_landing.then_some(heads.main));
    match call(schedule, &events, now, fix) {
        Call::Open(reason, at) => {
            let target = aim(&events, reason, heads.main, ready);
            let sha = match reason {
                Reason::FastPath => fix.unwrap_or(heads.main),
                Reason::Slot => target.as_str(),
            };
            pin::note(layout, &Event::Due { sha: sha.to_owned(), reason, at });
            println!("north runtime due: {} is promoted {}", layout::short(&target), if reason == Reason::Slot { format!("at the {} slot", clock(at)) } else { format!("at once, {} carries Live-fix", layout::short(sha)) });
            target
        }
        Call::Promote(reason) => aim(&events, reason, heads.main, ready),
        Call::Build(slot) => {
            if fs::read_to_string(layout.state.join(BUILT_FOR)).ok().and_then(|built| built.trim().parse().ok()) != Some(slot) {
                let _ = fs::write(layout.state.join(BUILT_FOR), format!("{slot}\n"));
                println!("north runtime: building {} ahead of the {} slot", layout::short(heads.main), clock(slot));
                build(heads.main);
            }
            waits(slot)
        }
        Call::Wait(slot) => waits(slot),
    }
}

fn aim(events: &[Event], reason: Reason, main: &str, ready: &dyn Fn(&str) -> bool) -> String {
    let newest_ready = || pin::waiting(events).into_iter().rev().map(|(sha, _)| sha).find(|sha| ready(sha));
    match reason {
        Reason::FastPath => main.to_owned(),
        Reason::Slot => newest_ready().unwrap_or(main).to_owned(),
    }
}

fn fix_since(checkout: &Path, pinned: &str, main: &str) -> Option<String> {
    let output = process::run("git", Some(checkout), &["log", "--format=%H%n%B%x1e", &format!("{pinned}..{main}")]).ok().filter(|output| output.status.success())?;
    let log = String::from_utf8_lossy(&output.stdout);
    log.split('\x1e').find_map(|commit| {
        let (sha, message) = commit.trim_start().split_once('\n')?;
        live_fix(message).map(|_| sha.to_owned())
    })
}

fn clock(slot: i64) -> String {
    let seconds = slot.rem_euclid(DAY);
    format!("{:02}:{:02} UTC", seconds / 3600, seconds % 3600 / 60)
}

const HAND_PIN: &str = "channel/stable";

pub(super) fn hand_pinned(state: &Path) -> bool {
    state.join(HAND_PIN).is_file()
}

pub(super) fn note_outcome(layout: &Layout, now: i64, outcome: Outcome) {
    record(layout, Schedule::declared(), now, outcome, None);
}

pub(super) fn note_failure(layout: &Layout, now: i64, sha: &str, probe: &str) {
    record(layout, Schedule::declared(), now, Outcome::Failed { probe: probe.to_owned() }, Some(sha));
}

const WORDS_MAX: usize = 200;

fn one_line(text: String) -> String {
    text.lines().next().unwrap_or_default().chars().take(WORDS_MAX).collect()
}

fn record(layout: &Layout, schedule: Schedule, now: i64, outcome: Outcome, failed_sha: Option<&str>) {
    let outcome = match outcome {
        Outcome::Refused { reason } => Outcome::Refused { reason: one_line(reason) },
        Outcome::Failed { probe } => Outcome::Failed { probe: one_line(probe) },
        promoted => promoted,
    };
    let events = pin::read(&layout.state);
    let due = || pin::owed(&events).into_iter().find(|&(_, reason, _)| reason == Reason::Slot).map(|(_, _, at)| at);
    let slot = match outcome {
        Outcome::Promoted => due(),
        Outcome::Refused { .. } => due().or_else(|| pin::waiting_since(&events).filter(|&landed| landed <= schedule.slot(now)).map(|_| schedule.slot(now))),
        Outcome::Failed { .. } => events.iter().rev().find_map(|event| match event {
            Event::Due { reason: Reason::Slot, at, sha } => Some(Some(*at).filter(|_| failed_sha == Some(sha.as_str()))),
            Event::Due { .. } => Some(None),
            _ => None,
        }).flatten(),
    };
    let Some(slot) = slot else { return };
    if last_slot(&events).is_some_and(|(held, _, last)| held == slot && *last == outcome) {
        return;
    }
    pin::note(layout, &Event::Slotted { slot, outcome, at: now });
}

fn last_slot(events: &[Event]) -> Option<(i64, i64, &Outcome)> {
    events.iter().rev().find_map(|event| match event {
        Event::Slotted { slot, outcome, at } => Some((*slot, *at, outcome)),
        _ => None,
    })
}

pub(super) fn status(state: &Path, now: i64) -> String {
    lines(Schedule::declared(), &pin::events_here_or_mirrored(state, "north runtime status"), now, hand_pinned(state))
}

fn lines(schedule: Schedule, events: &[Event], now: i64, hand_pin: bool) -> String {
    let recent = |at: i64| at > now - DAY;
    let promotions = events.iter().filter(|event| matches!(event, Event::Promoted { at, .. } if recent(*at))).count();
    let fast = events.iter().filter(|event| matches!(event, Event::Due { reason: Reason::FastPath, at, .. } if recent(*at))).count();
    let next = schedule.next_free(events, now);
    let last = last_slot(events).map_or_else(|| "last slot: none recorded\n".to_owned(), |(slot, at, outcome)| format!("last slot: {} (unix {slot}), {outcome} at {} (unix {at})\n", pin::iso(slot), pin::iso(at)));
    let held = if hand_pin { format!("no slot promotes until the hand-order pin ({HAND_PIN}, kept by north-stable-hold.service) is released\n") } else { String::new() };
    format!("promotions in 24 h: {promotions}\nfast paths in 24 h: {fast}\n{last}{held}next slot: {} (unix {next})\n", pin::iso(next))
}

#[cfg(test)]
mod tests;
