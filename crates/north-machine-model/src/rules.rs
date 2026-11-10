use std::collections::{BTreeMap, BTreeSet, VecDeque};

use north_base::lifecycle::{Lifecycle, Move, Record, State};

use super::world::{ACTORS, Event, Played, Situation, label, told};
use super::{Arrow, Finding, Moved, Promises};

const GONE: &str = "the holder,";

pub(crate) struct Step {
    pub(crate) before: Situation,
    pub(crate) event: Event,
    pub(crate) after: Situation,
}

fn named<S: State>(state: Option<S>) -> &'static str {
    state.map_or("no record", State::name)
}

fn finding(rule: &'static str, text: String) -> Finding {
    Finding { rule, text, after: Vec::new() }
}

fn gone(moved: &Move) -> bool {
    moved.cause.starts_with(GONE)
}

/// What the table alone breaks: an end with a way out, a dwell or a holder, a state with no way out that is no end,
/// and a state whose repeat the table and the promise disagree on.
pub(crate) fn table<L: Lifecycle>(promises: &Promises<L::State>) -> Vec<Finding> {
    L::State::ALL.iter().flat_map(|state| ends::<L>(*state, promises).into_iter().chain(repeats::<L>(*state, promises))).collect()
}

fn ends<L: Lifecycle>(state: L::State, promises: &Promises<L::State>) -> Vec<Finding> {
    let name = state.name();
    let exits: Vec<&str> = L::State::ALL.iter().filter(|to| **to != state && L::legal(Some(state), **to)).map(|to| to.name()).collect();
    let held = L::orphan(state).is_some();
    if !promises.ends.contains(&state) {
        let stuck = !held && exits.is_empty();
        return stuck.then(|| finding("stuck", format!("{name} has no move out and is not promised as an end"))).into_iter().collect();
    }
    let faults = [
        (!exits.is_empty(), format!("the table lets it move to {}", exits.join(", "))),
        (L::dwell(state).is_some(), "it has a dwell, so every instance that ends there turns overdue".to_owned()),
        (held, "a process holds it, so its holder's death moves it on".to_owned()),
    ];
    faults.into_iter().filter(|(broken, _)| *broken).map(|(_, why)| finding("ends", format!("{name} is promised as an end and {why}"))).collect()
}

fn repeats<L: Lifecycle>(state: L::State, promises: &Promises<L::State>) -> Option<Finding> {
    let name = state.name();
    match (L::legal(Some(state), state), promises.repeats.contains(&state)) {
        (true, false) => Some(finding("repeats", format!("the table lets a writer announce {name} again and the machine does not promise that"))),
        (false, true) => Some(finding("repeats", format!("{name} is promised to be announced again and the table refuses it, so the second writer fails"))),
        _ => None,
    }
}

/// Whether the instance's log alone rebuilds its record: every logged move starts where the one before it ended and
/// is one the table allows or the store's own move for a holder that is gone, every refused move is one the table
/// refuses, and the record's state and move count are the log's.
fn logged<L: Lifecycle>(log: &[Move], record: Option<&Record<L::State>>) -> Vec<String> {
    let mut broken = Vec::new();
    let (mut state, mut accepted) = (None::<L::State>, 0_i64);
    for moved in log {
        let Some(to) = L::State::parse(&moved.to) else {
            broken.push(format!("the log names {}, which is no state of the machine", moved.to));
            continue;
        };
        broken.extend(entry::<L>(moved, state, to));
        if !moved.refused {
            (state, accepted) = (Some(to), accepted + 1);
        }
    }
    let kept = record.map(|record| record.state);
    if kept != state {
        broken.push(format!("the record says {} and the log ends at {}", named(kept), named(state)));
    }
    if let Some(record) = record.filter(|record| record.seq != accepted) {
        broken.push(format!("the record counts {} moves and the log holds {accepted}", record.seq));
    }
    broken
}

fn entry<L: Lifecycle>(moved: &Move, state: Option<L::State>, to: L::State) -> Vec<String> {
    let mut broken = Vec::new();
    let from = moved.from.as_deref().and_then(L::State::parse);
    if from != state {
        broken.push(format!("the log has a move out of {} while the instance was in {}", named(from), named(state)));
    }
    let allowed = L::legal(state, to) || (gone(moved) && state.and_then(L::orphan) == Some(to));
    if moved.refused == allowed {
        let verdict = if moved.refused { "refused a move the table allows" } else { "accepted a move the table refuses" };
        broken.push(format!("the store {verdict}, from {} to {}", named(state), to.name()));
    }
    broken
}

/// The arrows the step's last event drew, and what it broke: a log that does not rebuild the record, a live holder
/// declared gone, or a held state taken by a move the machine does not let replace a holder.
pub(crate) fn step<L: Lifecycle>(step: &Step, played: &Played<L::State>) -> (Vec<Moved>, Vec<Finding>) {
    let mut findings: Vec<Finding> = logged::<L>(&played.log, played.record.as_ref()).into_iter().map(|text| finding("logged", text)).collect();
    let mut arrows = Vec::new();
    let taker = step.before.holder.zip(step.event.by()).filter(|(holder, by)| holder != by);
    let orphaned = played.fresh().iter().any(|moved| !moved.refused && gone(moved));
    for moved in played.fresh().iter().filter(|moved| !moved.refused) {
        let (Some(to), from) = (L::State::parse(&moved.to), moved.from.as_deref().and_then(L::State::parse)) else { continue };
        let arrow = match (gone(moved), taker) {
            (true, _) => Arrow::HolderGone,
            (false, Some(_)) if !orphaned => Arrow::Replaces,
            _ => Arrow::Writer,
        };
        if let (Some(taker), Some(from)) = (taker, from) {
            findings.extend(taken::<L>(&step.before, taker, (from, to), &arrow));
        }
        arrows.push(Moved { from: from.map(State::name), to: to.name(), arrow });
    }
    (arrows, findings)
}

fn taken<L: Lifecycle>(before: &Situation, (holder, by): (usize, usize), (from, to): (L::State, L::State), arrow: &Arrow) -> Option<Finding> {
    let unseen = ACTORS[holder].0 != ACTORS[by].0 && before.overdue;
    match arrow {
        Arrow::HolderGone if !before.dead && !unseen => Some(finding("one-holder", format!("{} declared {}, a live process holding {}, gone", label(by), label(holder), from.name()))),
        Arrow::Replaces if !L::preempts(from, to) => {
            Some(finding("one-holder", format!("{} took {} from {} by moving it to {}, a move the machine does not let replace a holder", label(by), from.name(), label(holder), to.name())))
        }
        _ => None,
    }
}

/// What the explored graph breaks: a state no ordering reaches, and a situation outside the promised ends that no
/// ordering leaves once nothing more dies.
pub(crate) fn reached<L: Lifecycle>(promises: &Promises<L::State>, paths: &BTreeMap<Situation, Vec<Event>>, steps: &[Step]) -> Vec<Finding> {
    let states = L::State::ALL;
    let unreached = states.iter().enumerate().filter(|(index, _)| !paths.keys().any(|situation| situation.state == Some(*index)));
    let mut findings: Vec<Finding> = unreached.map(|(_, state)| finding("reach", format!("no ordering reaches {}", state.name()))).collect();
    let mut next: BTreeMap<Situation, Vec<Situation>> = BTreeMap::new();
    for step in steps.iter().filter(|step| step.event != Event::Die) {
        next.entry(step.before).or_default().push(step.after);
    }
    for (situation, path) in paths {
        let Some(state) = situation.state.filter(|state| !promises.ends.contains(&states[*state])) else { continue };
        if !leaves(*situation, &next) {
            let holder = situation.holder.map_or_else(String::new, |holder| format!(" held by {}{}", label(holder), if situation.dead { ", which is dead" } else { "" }));
            findings.push(Finding { rule: "stuck", text: format!("no ordering moves the instance out of {}{holder}", states[state].name()), after: told::<L>(path) });
        }
    }
    findings
}

fn leaves(from: Situation, next: &BTreeMap<Situation, Vec<Situation>>) -> bool {
    let mut seen = BTreeSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(situation) = queue.pop_front() {
        for after in next.get(&situation).into_iter().flatten() {
            if after.state != from.state {
                return true;
            }
            if seen.insert(*after) {
                queue.push_back(*after);
            }
        }
    }
    false
}
