//! The share of code-change briefs that carried the full grounding pack, read from the first prompt
//! the ledger keeps for each session: the bar the launch-side change in north#1147 is measured by.
use std::collections::{BTreeMap, HashMap};

use north_pace::router::brief::{GROUNDING_PACK, PLAIN_ARM_LINE};

use super::Run;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Share {
    /// Code-change briefs in the window that the ledger kept a first prompt for.
    pub briefs: usize,
    /// Those whose first prompt carries the pack.
    pub carried: usize,
}

impl Share {
    pub fn percent(&self) -> f64 {
        if self.briefs == 0 {
            return 0.0;
        }
        100.0 * self.carried as f64 / self.briefs as f64
    }

    /// Whether the share clears `bar` percent, with at least one brief to read.
    pub fn meets(&self, bar: f64) -> bool {
        self.briefs > 0 && self.percent() >= bar
    }
}

/// The share over the sessions whose first recorded run started at or after `since_ms`.
pub fn share(runs: &[Run], first_prompts: &HashMap<String, String>, since_ms: i64) -> Share {
    let mut started: BTreeMap<&str, i64> = BTreeMap::new();
    for run in runs {
        started.entry(run.session.as_str()).and_modify(|at| *at = (*at).min(run.started_ms)).or_insert(run.started_ms);
    }
    let mut share = Share::default();
    for (session, at) in started {
        if at < since_ms {
            continue;
        }
        let Some(prompt) = first_prompts.get(session) else { continue };
        if !north_pace::router::brief::code_change(prompt) || prompt.lines().next() == Some(PLAIN_ARM_LINE) {
            continue;
        }
        share.briefs += 1;
        if prompt.contains(GROUNDING_PACK) {
            share.carried += 1;
        }
    }
    share
}
