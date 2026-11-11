//! Budget notices (north#259 box 10b): each tick compares what a box and its workstream have spent with the E their
//! briefs stated, records warnings, velocity notes and outcomes, and tells the sessions and leads concerned. A notice
//! stops nothing: it asks for a one-line check and the work goes on.
mod assess;
mod state;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

pub use assess::{Assessment, Scope, Spend};
pub use state::State;

#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub p50: Option<f64>,
    pub p90: f64,
    pub minutes: f64,
}

/// One session as a budget notice sees it: what its brief said E was and what it has spent.
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
    pub estimate: Option<Estimate>,
    pub landed: u32,
}

impl Spending {
    fn expected(&self) -> Option<Estimate> {
        let boxes = f64::from(self.landed) + 1.0;
        self.estimate.as_ref().map(|estimate| Estimate { p50: estimate.p50.map(|p50| boxes * p50), p90: boxes * estimate.p90, minutes: boxes * estimate.minutes })
    }
}

/// A line of the budget ledger: a warning, a velocity note, or the outcome of a box or workstream that ended.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Event {
    pub at_ms: u64,
    pub kind: &'static str,
    pub scope: String,
    pub spent: f64,
    pub p50: f64,
    pub p90: f64,
    pub ended: Option<Assessment>,
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

pub fn judge(now_ms: u64, sessions: &[Spending], previous: &State) -> Judged {
    let assessed = assess::spend(sessions, &previous.spends());
    let mut judged = Judged::default();
    for (scope, spend) in assessed.iter().filter(|(_, spend)| spend.changed) {
        let kind = match spend.confirmed {
            Assessment::Warn => "warn",
            Assessment::Velocity => "velocity",
            Assessment::Unestimated | Assessment::Within => continue,
        };
        let event = Event { at_ms: now_ms, kind, scope: state::key(scope), spent: spend.spent, p50: spend.p50, p90: spend.p90, ended: None };
        if speaks(scope, sessions) {
            note(&mut judged, sessions, &event, scope);
        }
        judged.events.push(event);
    }
    for (gone, seen) in previous.scopes.iter().filter(|(key, _)| !assessed.keys().any(|scope| &state::key(scope) == *key)) {
        judged.events.push(Event { at_ms: now_ms, kind: "outcome", scope: gone.clone(), spent: seen.spent, p50: seen.p50, p90: seen.p90, ended: Some(seen.confirmed) });
    }
    judged.state = State::remembering(&assessed);
    judged
}

/// Whether a scope's changes reach Tom, its leads and its sessions: every workstream, and a box when its workstream
/// holds others, since there the workstream sums every box's E and one box past its own p90 would pass unheard.
fn speaks(scope: &Scope, sessions: &[Spending]) -> bool {
    match scope {
        Scope::Workstream(_) => true,
        Scope::Box(id) => sessions.iter().find(|spending| &spending.box_id == id).is_some_and(|spending| boxes(sessions, &spending.workstream).len() > 1),
    }
}

fn named(scope: &Scope) -> &str {
    match scope {
        Scope::Workstream(name) | Scope::Box(name) => name,
    }
}

fn within(spending: &Spending, scope: &Scope) -> bool {
    match scope {
        Scope::Workstream(workstream) => &spending.workstream == workstream,
        Scope::Box(id) => &spending.box_id == id,
    }
}

fn note(judged: &mut Judged, sessions: &[Spending], event: &Event, scope: &Scope) {
    let name = named(scope);
    let who = |spending: &&Spending| within(spending, scope) && spending.live && !spending.tom;
    let leads: BTreeSet<&str> = sessions.iter().filter(who).filter_map(|spending| spending.parent.as_deref()).collect();
    let line = format!("{name} has spent ${:.2} against E p50 ${:.2}, p90 ${:.2}", event.spent, event.p50, event.p90);
    match event.kind {
        "warn" => {
            judged.alerts.push(format!("Budget warning: {line}, 2x its p90."));
            judged.tells.extend(leads.iter().map(|lead| Tell { to: (*lead).to_owned(), text: format!("[north: budget] {line}, past 2x its p90; check the box is still on course in one line, then continue your work in this turn.") }));
        }
        "velocity" => {
            let workers = sessions.iter().filter(who).filter(|spending| !spending.lead).map(|spending| spending.name.as_str());
            judged.tells.extend(workers.map(|worker| Tell { to: worker.to_owned(), text: format!("[north: budget] {line}, past half of p50 in under a quarter of the expected minutes; check your approach still fits the box's budget in one line, then continue your work in this turn.") }));
            if event.spent >= event.p90 {
                judged.tells.extend(leads.iter().map(|lead| Tell { to: (*lead).to_owned(), text: format!("[north: budget] {line}, past its p90 in under a quarter of the expected minutes.") }));
            }
        }
        _ => {}
    }
}

fn boxes<'a>(sessions: &'a [Spending], workstream: &str) -> BTreeMap<&'a String, f64> {
    let mut boxes: BTreeMap<&String, f64> = BTreeMap::new();
    for spending in sessions.iter().filter(|spending| spending.workstream == workstream && !spending.tom && !spending.lead) {
        let p90 = spending.expected().map_or(0.0, |estimate| estimate.p90);
        let held = boxes.entry(&spending.box_id).or_default();
        *held = held.max(p90);
    }
    boxes
}
