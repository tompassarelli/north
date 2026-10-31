use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;

use super::model::{Delivery, SessionRecord, Status};
use super::registry::{Fleet, HandedIn};
use super::store::{FleetRow, RowKind, WaitKey, WaitKind, WaitRow};
use super::turns::TurnExit;
use super::wait::ended_on_a_wait;

pub const WAKES: u32 = 3;
pub const GRACE_TICKS: u32 = 2;
pub const JUDGED_FOR: Duration = Duration::from_secs(48 * 3600);
pub const WAKES_PER_TICK: usize = 4;
const QUOTE_CHARS: usize = 240;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LastTurn {
    pub reply: String,
    pub id: u64,
    pub ended_ms: u64,
    pub exit: TurnExit,
}

#[derive(Clone, Debug)]
pub struct Worker<'a> {
    pub running: bool,
    pub gated: bool,
    pub turn: Option<LastTurn>,
    pub waits: Vec<&'a WaitRow>,
    pub registered_here: bool,
    pub pull_failed: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Mark {
    pub turn: u64,
    pub unwaited: u32,
    pub wakes: u32,
    pub acted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Verdict {
    Holds,
    Grace,
    Wake { reason: String, settled: Vec<WaitKey> },
    Stalled { settled: Vec<WaitKey> },
}

pub fn judge(worker: &Worker, mark: Mark, now: Timestamp) -> (Verdict, Mark) {
    let Some(turn) = worker.turn.as_ref().filter(|turn| turn.exit == TurnExit::Completed) else { return (Verdict::Holds, mark) };
    if worker.running || worker.gated {
        return (Verdict::Holds, mark);
    }
    if !ended_on_a_wait(&turn.reply) || !recent(turn, now) {
        return (Verdict::Holds, Mark::default());
    }
    let mark = if mark.turn == turn.id { mark } else { Mark { turn: turn.id, unwaited: 0, acted: false, ..mark } };
    if mark.acted {
        return (Verdict::Holds, mark);
    }
    let over: Vec<&WaitRow> = worker.waits.iter().copied().filter(|wait| !(wait.kind == WaitKind::Job && worker.pull_failed) && passed(wait, now)).collect();
    let (reason, settled) = if !over.is_empty() {
        (over.iter().map(|wait| reason(wait)).collect::<Vec<_>>().join("; "), over.iter().map(|wait| wait.key()).collect())
    } else if !worker.waits.is_empty() || worker.registered_here {
        return (Verdict::Holds, Mark { turn: mark.turn, ..Mark::default() });
    } else if mark.unwaited < GRACE_TICKS {
        return (Verdict::Grace, Mark { unwaited: mark.unwaited + 1, ..mark });
    } else {
        ("woken: parked with no registered wait".to_owned(), Vec::new())
    };
    let acted = Mark { acted: true, ..mark };
    if mark.wakes >= WAKES {
        return (Verdict::Stalled { settled }, acted);
    }
    (Verdict::Wake { reason, settled }, Mark { wakes: mark.wakes + 1, ..acted })
}

fn recent(turn: &LastTurn, now: Timestamp) -> bool {
    let age = now.unix_millis().saturating_sub(i64::try_from(turn.ended_ms).unwrap_or(i64::MAX));
    age < i64::try_from(JUDGED_FOR.as_millis()).unwrap_or(i64::MAX)
}

fn passed(wait: &WaitRow, now: Timestamp) -> bool {
    Timestamp::parse(&wait.deadline).is_ok_and(|deadline| deadline <= now)
}

fn reason(wait: &WaitRow) -> String {
    match wait.kind {
        WaitKind::Job => format!("woken: your job {}'s lease lapsed with no exit", wait.key),
        kind => format!("woken: wait {}:{} passed its deadline", kind.label(), wait.key),
    }
}

pub fn wake_prompt(reason: &str, wake: u32) -> String {
    format!("[north: {reason}; wake {wake} of {WAKES}] Read where your work stands and continue to a Done, Not done, Split, Stop or Blocked report. If a run is still going, register it with `job-run --notify` or `north fleet wait` before you end your turn.")
}

pub fn stalled(child: &str, reply: &str) -> String {
    let quoted: String = reply.trim().chars().take(QUOTE_CHARS).collect();
    format!("[from {child} · ended on a wait · woken {WAKES} times without a final report] Last reply: \"{quoted}\". Read it with `north fleet show {child}`.")
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Kept {
    pub woken: Vec<(String, String)>,
    pub stalled: Vec<String>,
    pub settled: Vec<WaitKey>,
    pub failed: Vec<(String, String)>,
}

impl Kept {
    pub fn lines(&self) -> Vec<String> {
        let woken = self.woken.iter().map(|(name, reason)| format!("{reason}: {name}"));
        let stalled = self.stalled.iter().map(|name| format!("stalled: {name} woken {WAKES} times, its parent told"));
        let failed = self.failed.iter().map(|(name, why)| format!("not kept {name}: {why}"));
        woken.chain(stalled).chain(failed).collect()
    }
}

pub struct Facts<'a> {
    pub waits: &'a [WaitRow],
    pub now: Timestamp,
    pub pull_failed: &'a [String],
    pub gated: &'a [String],
}

pub fn keep(fleet: &Fleet, records: &[SessionRecord], facts: &Facts) -> Kept {
    let mut kept = Kept::default();
    for record in records.iter().filter(|record| record.task_ref.is_some() && !record.stopped) {
        if kept.woken.len() + kept.stalled.len() >= WAKES_PER_TICK {
            break;
        }
        if let Err(error) = keep_one(fleet, record, facts, &mut kept) {
            kept.failed.push((record.name.clone(), error.user_message()));
        }
    }
    kept
}

fn keep_one(fleet: &Fleet, record: &SessionRecord, facts: &Facts, kept: &mut Kept) -> NorthResult<()> {
    if !fleet.active_within(record, JUDGED_FOR) {
        return Ok(());
    }
    let running = matches!(fleet.status(record), Status::Starting | Status::Working);
    let turn = if running { None } else { fleet.last_turn(record) };
    let worker = Worker {
        running,
        gated: facts.gated.contains(&record.name),
        turn,
        waits: facts.waits.iter().filter(|wait| wait.session == record.name).collect(),
        registered_here: !fleet.waits().of(&record.name).is_empty(),
        pull_failed: record.machine.as_ref().is_some_and(|machine| facts.pull_failed.contains(&machine.name)),
    };
    let before = fleet.invariant_mark(&record.name);
    let (verdict, after) = judge(&worker, before, facts.now);
    match verdict {
        Verdict::Holds | Verdict::Grace => {}
        Verdict::Wake { reason, settled } => {
            let ended_ms = worker.turn.as_ref().map_or(0, |turn| turn.ended_ms);
            let row = wake_row(fleet, &record.name, after, ended_ms, &reason);
            if fleet.hand_in(&row)? == HandedIn::Delivered {
                kept.woken.push((record.name.clone(), reason));
            }
            kept.settled.extend(settled);
        }
        Verdict::Stalled { settled } => {
            if let Some(parent) = record.parent.as_deref() {
                let reply = worker.turn.as_ref().map_or("", |turn| turn.reply.as_str());
                fleet.tell(parent, &stalled(&record.name, reply), Delivery::Queued);
            }
            kept.stalled.push(record.name.clone());
            kept.settled.extend(settled);
        }
    }
    if after != before {
        fleet.set_invariant_mark(&record.name, after)?;
    }
    Ok(())
}

fn wake_row(fleet: &Fleet, name: &str, mark: Mark, ended_ms: u64, reason: &str) -> FleetRow {
    let tag: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    FleetRow {
        id: format!("{ended_ms}-wake{}-{}-{tag}", mark.wakes, mark.turn),
        kind: RowKind::Message,
        to: name.to_owned(),
        from: fleet.host().to_owned(),
        urgent: false,
        sent_at: ended_ms,
        text: wake_prompt(reason, mark.wakes),
        sender: None,
        gate: None,
        answer: None,
        job: None,
    }
}

#[cfg(test)]
#[path = "invariant_tests.rs"]
mod tests;
