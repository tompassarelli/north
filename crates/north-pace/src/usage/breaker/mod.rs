//! The spend breaker's tick (north#259 box 10b): `decide::assess` judges each box's and workstream's spend against the E its
//! brief stated, this module turns the judgement into what the tick does: record warnings, trips, clears and outcomes,
//! tell a tripped workstream's sessions once, and a tripped box's where its issue holds several boxes, and take clears
//! from newer briefs and Tom's gate answers.
mod ledger;
mod state;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

pub use ledger::{FILE as LEDGER_FILE, read as read_trips, trips as trips_in};
pub use state::{Cleared, State};

use super::decide::world::{BoxId, Clear, EstimateSource, Expected, Label, ModelFamily, Name, Owner, Session, Settings, Standing, Unix, Usd, Workstream, World};
use super::decide::{Assessment, Assessments, Scope, assess};

#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub p50: Option<f64>,
    pub p90: f64,
    pub minutes: f64,
}

/// One session as the breaker sees it: what its brief said E was and what it has spent.
#[derive(Clone, Debug, PartialEq)]
pub struct Spending {
    pub name: String,
    pub parent: Option<String>,
    pub workstream: String,
    pub box_id: String,
    pub lead: bool,
    pub tom: bool,
    pub live: bool,
    pub spent: Option<f64>,
    pub minutes: f64,
    pub created_ms: u64,
    pub estimate: Option<Estimate>,
}

/// A closed gate's answer about a workstream, which clears it when it names a p90.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub workstream: String,
    pub at_ms: u64,
    pub by: String,
    pub text: String,
}

/// A line of the breaker ledger: a warning, trip, clear, velocity note or box outcome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Event {
    pub at_ms: u64,
    pub kind: &'static str,
    pub scope: String,
    pub spent: f64,
    pub p50: f64,
    pub p90: f64,
    pub by: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tell {
    pub to: String,
    pub text: String,
}

#[derive(Debug, Default, PartialEq)]
pub struct Judged {
    pub state: State,
    pub events: Vec<Event>,
    pub tells: Vec<Tell>,
    pub alerts: Vec<String>,
}

pub fn judge(now_ms: u64, sessions: &[Spending], answers: &[Answer], previous: &State) -> Judged {
    let mut state = previous.clone();
    state.clears.extend(new_clears(sessions, answers, previous));
    let counted: Vec<Session> = sessions.iter().map(session).collect();
    let settings = Settings { clears: state.clears.iter().map(|clear| Clear { workstream: Workstream(clear.workstream.clone()), p90: Usd(clear.p90), by: clear.by.clone() }).collect(), ..Settings::default() };
    let world = World { now: Unix(i64::try_from(now_ms / 1000).unwrap_or(i64::MAX)), accounts: &[], readings: &[], limits: &[], sessions: &counted, tripped: &[], credit: None, settings: &settings };
    let assessed = assess(&world, &Assessments { spend: previous.spends(), ..Assessments::default() });
    let mut judged = Judged::default();
    for (scope, spend) in assessed.spend.iter().filter(|(_, spend)| spend.changed) {
        let kind = match &spend.confirmed {
            Assessment::Tripped => "trip",
            Assessment::Warn => "warn",
            Assessment::Velocity => "velocity",
            Assessment::Cleared { .. } => "clear",
            Assessment::Unestimated | Assessment::Within => continue,
        };
        let by = match &spend.confirmed {
            Assessment::Cleared { by } => Some(by.clone()),
            _ => None,
        };
        let event = Event { at_ms: now_ms, kind, scope: state::key(scope), spent: spend.spent.0, p50: spend.p50.0, p90: spend.p90.0, by };
        if speaks(scope, sessions) {
            note(&mut judged, &mut state, sessions, &event, scope);
        }
        judged.events.push(event);
    }
    tell_the_tripped(&mut judged, &mut state, sessions, &assessed.spend);
    for gone in previous.scopes.keys().filter(|key| !assessed.spend.keys().any(|scope| &state::key(scope) == *key)) {
        let seen = &previous.scopes[gone];
        judged.events.push(Event { at_ms: now_ms, kind: "outcome", scope: gone.clone(), spent: seen.spent, p50: seen.p50, p90: seen.p90, by: Some(seen.confirmed.clone()) });
        if let Some(workstream) = gone.strip_prefix("workstream:") {
            state.tripped_at_ms.remove(workstream);
        }
        state.told.remove(gone);
    }
    state.remember(&assessed.spend);
    judged.state = state;
    judged
}

fn session(spending: &Spending) -> Session {
    let expected = spending.estimate.as_ref().map_or(
        Expected { p50: Usd(0.0), p90: Usd(0.0), minutes: 0.0, source: EstimateSource::None },
        |estimate| Expected { p50: Usd(estimate.p50.unwrap_or(estimate.p90)), p90: Usd(estimate.p90), minutes: estimate.minutes, source: EstimateSource::Budget(Usd(estimate.p90)) },
    );
    Session {
        name: Name(spending.name.clone()),
        owner: if spending.tom { Owner::Tom } else { Owner::Fleet },
        account: Label(String::new()),
        family: ModelFamily::Opus,
        workstream: Some(Workstream(spending.workstream.clone())),
        box_id: Some(BoxId(spending.box_id.clone())),
        standing: if spending.tom { Standing::Proxy } else if spending.lead { Standing::Lead } else { Standing::Worker },
        spent: spending.spent.map(Usd),
        active_minutes: spending.minutes,
        expected,
        parked_since: None,
    }
}

/// Whether a scope's changes reach Tom, its leads and its sessions: every workstream, and a box when its workstream
/// holds others, since there the workstream sums every box's E and one box past its own p90 would pass unheard.
fn speaks(scope: &Scope, sessions: &[Spending]) -> bool {
    match scope {
        Scope::Workstream(_) => true,
        Scope::Box(BoxId(id)) => sessions.iter().find(|spending| &spending.box_id == id).is_some_and(|spending| boxes(sessions, &spending.workstream).len() > 1),
    }
}

fn named(scope: &Scope) -> &str {
    match scope {
        Scope::Workstream(Workstream(name)) | Scope::Box(BoxId(name)) => name,
    }
}

fn within(spending: &Spending, scope: &Scope) -> bool {
    match scope {
        Scope::Workstream(Workstream(workstream)) => &spending.workstream == workstream,
        Scope::Box(BoxId(id)) => &spending.box_id == id,
    }
}

fn note(judged: &mut Judged, state: &mut State, sessions: &[Spending], event: &Event, scope: &Scope) {
    let name = named(scope);
    let who = |spending: &&Spending| within(spending, scope) && spending.live && !spending.tom;
    let leads: BTreeSet<&str> = sessions.iter().filter(who).filter_map(|spending| spending.parent.as_deref()).collect();
    let line = format!("{name} has spent ${:.2} against E p50 ${:.2}, p90 ${:.2}", event.spent, event.p50, event.p90);
    match event.kind {
        "trip" => {
            if let Scope::Workstream(Workstream(workstream)) = scope {
                state.tripped_at_ms.insert(workstream.clone(), event.at_ms);
            }
            judged.alerts.push(format!("Budget tripped: {line}, 4x its p90; its sessions are told to finish their step, commit and report Split: or Not done: Stop: budget."));
        }
        "warn" => {
            judged.alerts.push(format!("Budget warning: {line}, 2x its p90."));
            judged.tells.extend(leads.iter().map(|lead| Tell { to: (*lead).to_owned(), text: format!("[north: budget] {line}, past 2x its p90; check the box is still on course.") }));
        }
        "velocity" => {
            judged.tells.extend(leads.iter().map(|lead| Tell { to: (*lead).to_owned(), text: format!("[north: budget] {line}, past half of p50 in under a quarter of the expected minutes.") }));
        }
        "clear" => {
            if let Scope::Workstream(Workstream(workstream)) = scope {
                state.tripped_at_ms.remove(workstream);
            }
        }
        _ => {}
    }
}

fn tell_the_tripped(judged: &mut Judged, state: &mut State, sessions: &[Spending], spends: &BTreeMap<Scope, super::decide::Spend>) {
    for (scope, spend) in spends.iter().filter(|(scope, _)| speaks(scope, sessions)) {
        let told = state::key(scope);
        if spend.confirmed != Assessment::Tripped {
            state.told.remove(&told);
            continue;
        }
        if state.told.contains(&told) {
            continue;
        }
        let name = named(scope);
        let text = format!(
            "[north: budget] {name} has spent ${:.2}, past 4x its p90 ${:.2}. Finish your step, commit what passes, and report `Split:` or `Not done:` with `Stop: budget` and what you learned. A newer brief whose Budget line states a higher p90, or Tom's answer to a gate that names one, clears this.",
            spend.spent.0, spend.p90.0
        );
        judged.tells.extend(sessions.iter().filter(|spending| within(spending, scope) && spending.live && !spending.tom && !spending.lead).map(|spending| Tell { to: spending.name.clone(), text: text.clone() }));
        state.told.insert(told);
    }
}

/// The clears a workstream tripped before this tick takes now: a brief launched after the trip whose p90, with the p90 of
/// the workstream's other boxes, exceeds the tripped one, and a gate answer after the trip that names a higher p90.
fn new_clears(sessions: &[Spending], answers: &[Answer], previous: &State) -> Vec<Cleared> {
    let tripped: BTreeSet<&str> = sessions.iter().map(|spending| spending.workstream.as_str()).filter(|workstream| previous.tripped(workstream).is_some()).collect();
    let mut found = Vec::new();
    for workstream in tripped {
        let (Some(seen), Some(&since)) = (previous.tripped(workstream), previous.tripped_at_ms.get(workstream)) else { continue };
        let mut candidates: Vec<Cleared> = Vec::new();
        for newer in sessions.iter().filter(|spending| spending.workstream == workstream && !spending.tom && spending.created_ms > since) {
            let Some(estimate) = &newer.estimate else { continue };
            let others: f64 = boxes(sessions, workstream).into_iter().filter(|(id, _)| **id != newer.box_id).map(|(_, p90)| p90).sum();
            candidates.push(Cleared { workstream: workstream.to_owned(), p90: estimate.p90 + others, by: format!("the brief of {}", newer.name) });
        }
        for answer in answers.iter().filter(|answer| answer.workstream == workstream && answer.at_ms > since) {
            if let Some(p90) = p90_in(&answer.text) {
                candidates.push(Cleared { workstream: workstream.to_owned(), p90, by: format!("{}'s answer to a gate", answer.by) });
            }
        }
        let best = candidates.into_iter().filter(|candidate| candidate.p90 > seen.p90).max_by(|left, right| left.p90.total_cmp(&right.p90));
        found.extend(best.filter(|best| !previous.clears.iter().any(|clear| clear.workstream == workstream && clear.p90 >= best.p90)));
    }
    found
}

fn boxes<'a>(sessions: &'a [Spending], workstream: &str) -> BTreeMap<&'a String, f64> {
    let mut boxes: BTreeMap<&String, f64> = BTreeMap::new();
    for spending in sessions.iter().filter(|spending| spending.workstream == workstream && !spending.tom && !spending.lead) {
        let p90 = spending.estimate.as_ref().map_or(0.0, |estimate| estimate.p90);
        let held = boxes.entry(&spending.box_id).or_default();
        *held = held.max(p90);
    }
    boxes
}

/// The `p90 $Y` an answer names, if any.
pub fn p90_in(text: &str) -> Option<f64> {
    let rest = text.split_once("p90 $")?.1;
    rest.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.trim_end_matches('.').parse().ok()
}
