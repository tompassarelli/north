use std::collections::{BTreeMap, BTreeSet};

use super::world::{BoxId, EstimateSource, Owner, Session, Standing, Trip, Usd, Workstream, World};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Box(BoxId),
    Workstream(Workstream),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Assessment {
    Unestimated,
    Cleared { by: String },
    Tripped,
    Warn,
    Velocity,
    Within,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spend {
    pub raw: Assessment,
    pub confirmed: Assessment,
    pub changed: bool,
    pub spent: Usd,
    pub p50: Usd,
    pub p90: Usd,
}

#[derive(Debug, Clone, Default)]
struct Totals {
    spent: f64,
    p50: f64,
    p90: f64,
    minutes: f64,
    active_minutes: f64,
    estimated: bool,
}

pub fn spend(world: &World, previous: &BTreeMap<Scope, Spend>) -> BTreeMap<Scope, Spend> {
    let counted: Vec<&Session> = world.sessions.iter().filter(|session| session.owner != Owner::Tom).collect();
    let boxes = box_totals(&counted);
    let mut answers = BTreeMap::new();
    for (id, (workstream, totals)) in &boxes {
        answers.insert(Scope::Box(id.clone()), judge(world, workstream.as_ref(), totals, previous.get(&Scope::Box(id.clone()))));
    }
    let workstreams: BTreeSet<&Workstream> = counted.iter().filter_map(|session| session.workstream.as_ref()).collect();
    for workstream in workstreams {
        let mut sum = Totals::default();
        for (_, totals) in boxes.values().filter(|(owner, _)| owner.as_ref() == Some(workstream)) {
            sum = add(sum, totals);
        }
        let scope = Scope::Workstream(workstream.clone());
        let judged = judge(world, Some(workstream), &sum, previous.get(&scope));
        answers.insert(scope, judged);
    }
    answers
}

fn box_totals(sessions: &[&Session]) -> BTreeMap<BoxId, (Option<Workstream>, Totals)> {
    let mut boxes: BTreeMap<BoxId, (Option<Workstream>, Totals)> = BTreeMap::new();
    for session in sessions.iter().filter(|session| session.standing == Standing::Worker) {
        let Some(id) = &session.box_id else { continue };
        let (workstream, totals) = boxes.entry(id.clone()).or_default();
        if let Some(found) = &session.workstream
            && workstream.as_ref().is_none_or(|current| found < current)
        {
            workstream.clone_from(&session.workstream);
        }
        totals.spent += session.spent.map_or(0.0, |spent| spent.0);
        totals.active_minutes += session.active_minutes;
        totals.p50 = totals.p50.max(session.expected.p50.0);
        totals.p90 = totals.p90.max(session.expected.p90.0);
        totals.minutes = totals.minutes.max(session.expected.minutes);
        totals.estimated |= session.expected.source != EstimateSource::None;
    }
    boxes
}

fn add(left: Totals, right: &Totals) -> Totals {
    Totals {
        spent: left.spent + right.spent,
        p50: left.p50 + right.p50,
        p90: left.p90 + right.p90,
        minutes: left.minutes + right.minutes,
        active_minutes: left.active_minutes + right.active_minutes,
        estimated: left.estimated || right.estimated,
    }
}

fn judge(world: &World, workstream: Option<&Workstream>, totals: &Totals, previous: Option<&Spend>) -> Spend {
    let raw = classify(world, workstream, totals);
    let immediate = matches!(raw, Assessment::Unestimated | Assessment::Cleared { .. });
    let confirmed = match previous {
        _ if immediate => raw.clone(),
        Some(before) if before.raw == raw => raw.clone(),
        Some(before) => before.confirmed.clone(),
        None => Assessment::Within,
    };
    let changed = previous.map_or(confirmed != Assessment::Within, |before| before.confirmed != confirmed);
    Spend { raw, confirmed, changed, spent: Usd(totals.spent), p50: Usd(totals.p50), p90: Usd(in_force(world, workstream, totals.p90)) }
}

fn in_force(world: &World, workstream: Option<&Workstream>, p90: f64) -> f64 {
    clear_for(world, workstream).map_or(p90, |(clear_p90, _)| clear_p90)
}

fn clear_for<'a>(world: &'a World, workstream: Option<&Workstream>) -> Option<(f64, &'a str)> {
    let workstream = workstream?;
    world
        .settings
        .clears
        .iter()
        .filter(|clear| &clear.workstream == workstream)
        .max_by(|left, right| left.p90.0.total_cmp(&right.p90.0))
        .map(|clear| (clear.p90.0, clear.by.as_str()))
}

fn classify(world: &World, workstream: Option<&Workstream>, totals: &Totals) -> Assessment {
    if !totals.estimated || totals.p90 <= 0.0 {
        return Assessment::Unestimated;
    }
    let cleared = clear_for(world, workstream);
    let p90 = in_force(world, workstream, totals.p90);
    if let Some((_, by)) = cleared.filter(|(clear_p90, _)| totals.spent < 4.0 * clear_p90) {
        return Assessment::Cleared { by: by.to_owned() };
    }
    if totals.spent >= 4.0 * p90 {
        Assessment::Tripped
    } else if totals.spent >= 2.0 * p90 {
        Assessment::Warn
    } else if totals.spent >= 0.5 * totals.p50 && totals.active_minutes < 0.25 * totals.minutes {
        Assessment::Velocity
    } else {
        Assessment::Within
    }
}

pub fn trips(spend: &BTreeMap<Scope, Spend>) -> Vec<Trip> {
    spend
        .iter()
        .filter_map(|(scope, entry)| match scope {
            Scope::Workstream(workstream) if entry.confirmed == Assessment::Tripped => {
                Some(Trip { workstream: workstream.clone(), spent: entry.spent, p90: entry.p90 })
            }
            _ => None,
        })
        .collect()
}
