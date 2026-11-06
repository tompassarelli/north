use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::store::{Action, Graph, Told};
use crate::term::Term;

#[derive(Debug, Default)]
pub struct Telling {
    pub facts: Vec<Told>,
    pub replaces: Vec<Term>,
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
        let live = self.told()?;
        let have: HashSet<&Told> = live.iter().collect();
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
        for stale in live.iter().filter(|told| replaced.contains(&told.triple.t2) && !wanted.contains(told)) {
            batches.entry(&stale.writer).or_default().push(Action::retract(stale.triple.clone()));
            synced.retracted += 1;
        }
        let mut ordered: Vec<(&str, Vec<Action>)> = batches.into_iter().collect();
        ordered.sort_by(|a, b| a.0.cmp(b.0));
        let borrowed: Vec<(&str, &[Action])> = ordered.iter().map(|(writer, actions)| (*writer, actions.as_slice())).collect();
        self.apply_batches(&borrowed)?;
        Ok(synced)
    }
}

