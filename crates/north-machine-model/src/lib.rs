//! Checks a `north_base::lifecycle` machine over every ordering of a stated scope by driving the machine's own
//! `Lifecycle` table through the real `Store`, so the model is the code and cannot drift from it: three processes, two
//! on one host and one on another, each try every move for every holder and settle the instance, the process holding
//! the state dies, and its dwell runs out (docs/rationale/watch/2026-11-10-pstack-verification.md).
//!
//! Depends on `north-base` alone and knows no machine; the caller names each machine and its promises.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use north_base::error::NorthResult;
use north_base::lifecycle::{Lifecycle, State};

mod chart;
mod pair;
mod rules;
mod world;

pub use chart::page;
pub use pair::Together;
use rules::Step;
use world::{Event, START, Situation, World};

/// What a machine promises beyond its table, in its own states.
pub struct Promises<S: 'static> {
    /// States an instance never leaves for another state.
    pub ends: &'static [S],
    /// States a writer may announce again while the instance is in them.
    pub repeats: &'static [S],
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Arrow {
    /// A writer's move.
    Writer,
    /// The store's move when the process holding the state is gone.
    HolderGone,
    /// A writer's move that took the state from a live holder.
    Replaces,
    /// The sweep's move when the state's deadline passed.
    Expired,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Moved {
    pub from: Option<&'static str>,
    pub to: &'static str,
    pub arrow: Arrow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub name: &'static str,
    pub dwell: Option<Duration>,
    pub held: bool,
    pub end: bool,
    pub repeats: bool,
    /// The state the sweep moves an instance to once its dwell here runs out.
    pub expires: Option<&'static str>,
}

/// A promise the machine broke, with the shortest ordering that shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub rule: &'static str,
    pub text: String,
    pub after: Vec<String>,
}

impl Finding {
    pub fn line(&self) -> String {
        if self.after.is_empty() { format!("{}: {}", self.rule, self.text) } else { format!("{}: {} (after: {})", self.rule, self.text, self.after.join("; ")) }
    }
}

/// One machine as explored: its states, every move the store accepted and every promise it broke.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub machine: &'static str,
    pub states: Vec<Facts>,
    pub moves: BTreeSet<Moved>,
    pub situations: usize,
    pub steps: usize,
    /// Instances the search covered: one, or two where a promise between two instances is stated.
    pub instances: usize,
    pub findings: Vec<Finding>,
}

struct Search<L: Lifecycle> {
    world: World<L>,
    paths: BTreeMap<Situation, Vec<Event>>,
    steps: Vec<Step>,
    moves: BTreeSet<Moved>,
    findings: Vec<Finding>,
}

impl<L: Lifecycle> Search<L> {
    fn step(&mut self, before: Situation, event: Event) -> NorthResult<Option<Situation>> {
        let mut path = self.paths[&before].clone();
        path.push(event);
        let played = self.world.play(&path)?;
        let step = Step { before, event, after: played.situation };
        let (arrows, findings) = rules::step::<L>(&step, &played);
        self.moves.extend(arrows);
        for mut finding in findings {
            if !self.findings.iter().any(|known| known.rule == finding.rule && known.text == finding.text) {
                finding.after = world::told::<L>(&path);
                self.findings.push(finding);
            }
        }
        self.steps.push(step);
        if self.paths.contains_key(&played.situation) {
            return Ok(None);
        }
        self.paths.insert(played.situation, path);
        Ok(Some(played.situation))
    }
}

/// Explores `L` from an instance with no record through every ordering of the scope and judges it against its table,
/// the store's log and `promises`.
pub fn explore<L: Lifecycle>(promises: &Promises<L::State>) -> NorthResult<Model> {
    let mut search = Search::<L> { world: World::open()?, paths: BTreeMap::from([(START, Vec::new())]), steps: Vec::new(), moves: BTreeSet::new(), findings: rules::table::<L>(promises) };
    let mut queue = VecDeque::from([START]);
    while let Some(before) = queue.pop_front() {
        search.world = World::open()?;
        for event in world::events::<L>(&before) {
            queue.extend(search.step(before, event)?);
        }
    }
    search.findings.extend(rules::reached::<L>(promises, &search.paths, &search.steps));
    let states = L::State::ALL
        .iter()
        .map(|state| Facts { name: state.name(), dwell: L::dwell(*state), held: L::orphan(*state).is_some(), end: promises.ends.contains(state), repeats: promises.repeats.contains(state), expires: L::expire(*state).map(|expiry| expiry.to.name()) })
        .collect();
    Ok(Model { machine: L::NAME, states, moves: search.moves, situations: search.paths.len(), steps: search.steps.len(), instances: 1, findings: search.findings })
}

/// Explores one instance of `L` and then two instances of it together, so a promise between two instances (one live
/// session per name, one queued ticket per withdrawn head) is stated and checked by the machine's real admission path.
pub fn explore_together<L: Lifecycle>(promises: &Promises<L::State>, together: &Together<L::State>) -> NorthResult<Model> {
    let mut model = explore::<L>(promises)?;
    let pair = pair::explore::<L>(together)?;
    model.moves.extend(pair.moves);
    model.situations += pair.situations;
    model.steps += pair.steps;
    model.instances = 2;
    model.findings.extend(pair.findings);
    Ok(model)
}
