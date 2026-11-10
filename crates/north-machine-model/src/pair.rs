//! Two instances of one machine explored together: the first through every ordering of the one-instance scope, the
//! second attempting to start at each of the first's situations, and two launches racing from no record. The
//! machine's real admission path decides whether a start is possible, and the promise states the situations in which
//! no second instance may start.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use north_base::error::NorthResult;
use north_base::lifecycle::{Lifecycle, State};

use crate::world::{Event, PairEvent, PairSituation, PairWorld, START};
use crate::{Finding, Moved};

pub struct Together<S: 'static> {
    /// Situations in which one instance holds the name or head, so no second instance may start.
    pub alone: &'static [S],
    /// Whether the machine's real admission path admits a start while the other instance is in this state.
    pub admits: fn(S) -> bool,
    /// How many of two launches racing from no record the machine's real name claim admits.
    pub race: fn() -> usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    One { side: usize, event: Event },
    Race { left: usize, right: usize },
}

pub struct Outcome {
    pub findings: Vec<Finding>,
    pub moves: BTreeSet<Moved>,
    pub situations: usize,
    pub steps: usize,
}

fn starts<L: Lifecycle>(side: usize) -> Vec<PairEvent> {
    L::State::ALL
        .iter()
        .enumerate()
        .filter(|(_, state)| L::legal(None, **state))
        .map(|(to, _)| PairEvent::One { side, event: Event::Move { by: side, to, holder: side } })
        .collect()
}

fn events<L: Lifecycle>(situation: &PairSituation, together: &Together<L::State>) -> Vec<PairEvent> {
    let mut events = Vec::new();
    let dead = situation.sides.iter().find(|side| side.dead).and_then(|side| side.holder);
    for event in crate::world::events::<L>(&situation.sides[0]) {
        if event.by().is_none_or(|by| Some(by) != dead) {
            events.push(PairEvent::One { side: 0, event });
        }
    }
    if situation.sides[1].state.is_none()
        && let Some(other) = situation.sides[0].state
        && (together.admits)(L::State::ALL[other])
    {
        events.extend(starts::<L>(1));
    }
    if situation.sides[0].state.is_none() && situation.sides[1].state.is_none() && (together.race)() == 2 {
        let reachable: Vec<usize> = L::State::ALL.iter().enumerate().filter(|(_, state)| L::legal(None, **state)).map(|(index, _)| index).collect();
        for left in reachable.iter().copied() {
            for right in reachable.iter().copied() {
                events.push(PairEvent::Race { left, right });
            }
        }
    }
    events
}

fn step_of(event: PairEvent) -> Step {
    match event {
        PairEvent::One { side, event } => Step::One { side, event },
        PairEvent::Race { left, right } => Step::Race { left, right },
    }
}

fn told<L: Lifecycle>(path: &[PairEvent]) -> Vec<String> {
    path.iter()
        .map(|event| match *event {
            PairEvent::One { side, event } => format!("the {} instance: {}", if side == 0 { "first" } else { "second" }, event.told::<L>()),
            PairEvent::Race { .. } => "two launches claimed the name at once".to_owned(),
        })
        .collect()
}

fn event_of(step: Step, side: usize) -> Event {
    match step {
        Step::One { event, .. } => event,
        Step::Race { left, right } => Event::Move { by: side, to: if side == 0 { left } else { right }, holder: side },
    }
}

/// Explores two instances of `L` together and returns the promise a start broke, the arrows both drew, and the
/// situations and steps the search counted.
pub fn explore<L: Lifecycle>(together: &Together<L::State>) -> NorthResult<Outcome> {
    let start = PairSituation { sides: [START, START] };
    let mut paths: BTreeMap<PairSituation, Vec<PairEvent>> = BTreeMap::from([(start, Vec::new())]);
    let mut queue = VecDeque::from([start]);
    let mut findings: Vec<Finding> = Vec::new();
    let mut moves = BTreeSet::new();
    let (mut situations, mut steps) = (0_usize, 0_usize);
    while let Some(before) = queue.pop_front() {
        let mut world = PairWorld::<L>::open()?;
        for event in events::<L>(&before, together) {
            let step = step_of(event);
            let mut path = paths[&before].clone();
            path.push(event);
            let played = world.play(&path)?;
            steps += 1;
            for (side, transition) in played.moved.iter().enumerate() {
                let Some(transition) = transition else { continue };
                let rule_step = crate::rules::Step { before: before.sides[side], event: event_of(step, side), after: played.sides[side] };
                let (arrows, broken) = crate::rules::step::<L>(&rule_step, transition);
                moves.extend(arrows);
                for mut broken in broken {
                    if !findings.iter().any(|known| known.rule == broken.rule && known.text == broken.text) {
                        broken.after = told::<L>(&path);
                        findings.push(broken);
                    }
                }
            }
            let broken = match step {
                Step::One { side: 1, .. } => before.sides[0].state.map(|other| L::State::ALL[other]).filter(|other| together.alone.contains(other)).map(|other| {
                    format!("a second instance started while the first was {}, and the machine admits only one instance in it", other.name())
                }),
                Step::Race { .. } => {
                    let left = played.sides[0].state.map(|state| L::State::ALL[state]);
                    let right = played.sides[1].state.map(|state| L::State::ALL[state]);
                    left.zip(right).filter(|(left, right)| together.alone.contains(left) && together.alone.contains(right)).map(|(_, right)| {
                        format!("two launches started together and both hold the name, the second as {}", right.name())
                    })
                }
                _ => None,
            };
            if let Some(text) = broken
                && !findings.iter().any(|known| known.rule == "together" && known.text == text)
            {
                findings.push(Finding { rule: "together", text, after: told::<L>(&path) });
            }
            let after = PairSituation { sides: played.sides };
            if let std::collections::btree_map::Entry::Vacant(entry) = paths.entry(after) {
                entry.insert(path);
                queue.push_back(after);
                situations += 1;
            }
        }
    }
    Ok(Outcome { findings, moves, situations, steps })
}

#[cfg(test)]
#[path = "pair_tests.rs"]
mod tests;
