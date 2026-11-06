//! A candidate's score on the recorded decisions: strategy replay with the candidate applied to every
//! logged placement case it reaches, against replay under the current policy.
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use north_replay::{Bounds, Deciders};
use north_strategy_types::placement;

use crate::candidate::Change;

#[derive(Debug, PartialEq)]
pub struct Score {
    pub placement: String,
    pub reached: usize,
    pub refused_now: usize,
    pub refused_by_candidate: Vec<String>,
}

impl Score {
    pub fn passes(&self) -> bool {
        self.refused_by_candidate.is_empty()
    }
}

/// Replays `[now - since, now]` of the decision log under `root` twice, with the linked policy and with `change` applied.
pub fn replay(change: &Change, root: &Path, since: f64, now: f64) -> Score {
    let bounds = Bounds::from_defaults();
    let linked = Deciders::linked();
    let current = north_replay::replay(root, now - since, now, &linked, &bounds).refusals(&bounds, true);
    let reached = AtomicUsize::new(0);
    let patched = |case: &placement::Case| {
        let mut case = case.clone();
        if change.patch(&mut case) {
            reached.fetch_add(1, Ordering::Relaxed);
        }
        (linked.placement)(&case)
    };
    let candidate = Deciders { admission: linked.admission, judge: linked.judge, placement: &patched };
    let replayed = north_replay::replay(root, now - since, now, &candidate, &bounds);
    let refused_by_candidate = replayed.refusals(&bounds, true).into_iter().filter(|refusal| !current.contains(refusal)).collect();
    let placement = replayed.counts.iter().find(|counts| counts.kind == "placement").map_or_else(|| "placement: no cases".to_owned(), |counts| counts.line());
    Score { placement, reached: reached.into_inner(), refused_now: current.len(), refused_by_candidate }
}
