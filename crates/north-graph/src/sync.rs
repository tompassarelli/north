use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::store::{Action, Graph, Told};
use crate::term::{Term, Triple};

#[derive(Debug, Default)]
pub struct Telling {
    pub facts: Vec<Told>,
    pub replaces: Vec<Term>,
    pub held: Held,
}

#[derive(Debug, Default)]
pub struct Held {
    pub writer: String,
    pub subjects: Vec<Term>,
    pub predicates: Vec<Term>,
    pub quoting: Vec<Term>,
    pub widens: Option<fn(&Term, &Term) -> bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Synced {
    pub asserted: usize,
    pub retracted: usize,
    pub unchanged: usize,
}

impl Graph {
    pub fn sync(&mut self, sync: &Telling) -> Result<Synced> {
        let wanted: HashSet<&Told> = sync.facts.iter().collect();
        let replaced: HashSet<&Term> = sync.replaces.iter().collect();
        let claimed: HashSet<(&str, &Term)> = wanted.iter().map(|told| (told.writer.as_str(), &told.triple.t2)).collect();
        let live = self.told_within(&claimed, &replaced)?;
        let have: HashSet<&Told> = live.iter().collect();
        let holds: HashSet<&Term> = sync.held.predicates.iter().collect();
        let held_by = |told: &Told, subjects: &HashSet<&Term>| told.writer == sync.held.writer && holds.contains(&told.triple.t2) && subjects.contains(&told.triple.t1);
        let mut held: HashSet<&Term> = sync.held.subjects.iter().collect();
        let reached: Vec<&Term> = live.iter().filter(|told| held_by(told, &held)).map(|told| &told.triple.t3).collect();
        held.extend(reached);
        let quoting: HashSet<&Term> = sync.held.quoting.iter().collect();
        let backs = |stale: &Told, whole: &Triple, told: &Told| {
            told.writer == stale.writer && told.triple.t1 == stale.triple.t1 && told.triple.t2 == stale.triple.t2 && quoted(&quoting, told).is_some_and(|short| short.t2 == whole.t2 && short.t3 == whole.t3 && sync.held.widens.is_some_and(|widens| widens(&whole.t1, &short.t1)))
        };
        let quotes_held = |told: &Told| quoted(&quoting, told).is_some_and(|whole| held.contains(&whole.t1) && wanted.iter().any(|now| backs(told, whole, now)));
        let kept = |told: &Told| held_by(told, &held) || quotes_held(told);
        let mut batches: HashMap<&str, Vec<Action>> = HashMap::new();
        let mut synced = Synced::default();
        for fact in &wanted {
            if have.contains(fact) {
                synced.unchanged += 1;
            } else {
                batches.entry(&fact.writer).or_default().push(Action::assert(fact.triple.clone()));
                synced.asserted += 1;
            }
        }
        for stale in live.iter().filter(|told| replaced.contains(&told.triple.t2) && !wanted.contains(told) && !kept(told)) {
            batches.entry(&stale.writer).or_default().push(Action::retract(stale.triple.clone()));
            synced.retracted += 1;
        }
        let mut ordered: Vec<(&str, Vec<Action>)> = batches.into_iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(b.0));
        let borrowed: Vec<(&str, &[Action])> = ordered.iter().map(|(writer, actions)| (*writer, actions.as_slice())).collect();
        self.apply_batches(&borrowed)?;
        Ok(synced)
    }

    pub fn retire(&mut self, writer: &str) -> Result<usize> {
        let live = self.told_by(writer)?;
        let actions: Vec<Action> = live.into_iter().map(|told| Action::retract(told.triple)).collect();
        if !actions.is_empty() {
            self.apply(writer, &actions)?;
        }
        Ok(actions.len())
    }
}

fn quoted<'t>(quoting: &HashSet<&Term>, told: &'t Told) -> Option<&'t Triple> {
    if quoting.contains(&told.triple.t2) { told.triple.t3.as_triple() } else { None }
}
