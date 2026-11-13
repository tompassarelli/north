//! The culprit search a stopped lander lost, resumed from the red verdict its run recorded instead of rerunning the checks (north#1354).
use std::collections::BTreeMap;

use super::{AFTER, Batch, Cancel, Category, Lander, PHASE, Run, Searching, Span, Watch, heads, landed, moved, post_land, record, retire};
use super::post_land::Outcome;

impl Watch {
    /// Resumes the culprit search of each batch whose every head a stopped lander lost in its search; the rest rerun their checks.
    pub(super) fn resume_searches(&mut self, lander: &Lander, lost: &BTreeMap<String, f64>, resumed: f64) {
        let (searched, rest): (Vec<Batch>, Vec<Batch>) = std::mem::take(&mut self.pending.batches).into_iter().partition(|batch| heads(batch).all(|head| lost.contains_key(head)));
        self.pending.batches = rest;
        let mut again = Vec::new();
        for batch in searched {
            match post_land::resume(lander, landed(lander, &batch)) {
                Some(Outcome { search: Some(search), verdict, log, failing, superseded }) => {
                    println!("north land: resuming the culprit search of {}, which a stopped lander left; its checks are not rerun", batch.head);
                    moved(heads(&batch), Run::Running, "resumed the red verdict a stopped lander recorded");
                    moved(heads(&batch), Run::Searching, "the culprit search a stopped lander left resumes");
                    let outcome = Outcome { verdict, search: None, log, failing, superseded };
                    self.searching.push(Searching { batch, outcome, search: post_land::search(&lander.wake, search), host: None, cancel: Cancel::default() });
                }
                Some(_) => {
                    println!("north land: the red verdict of {} a stopped lander recorded was covered by a descendant's green run, so no lane is blamed", batch.head);
                    moved(heads(&batch), Run::Running, "resumed the red verdict a stopped lander recorded");
                    retire(lander, &batch, "a descendant's green run covered its red verdict");
                }
                None => again.push(batch),
            }
        }
        for batch in again {
            batch.heads.iter().for_each(|followed| rerun(lander, &followed.head, lost.get(&followed.head).copied().unwrap_or(resumed), resumed));
            self.pending.add(batch);
        }
    }
}

pub(super) fn rerun(lander: &Lander, head: &str, at: f64, resumed: f64) {
    println!("north land: rerunning the after-landing run of {head}, which a stopped lander left without a verdict");
    record(lander, &[Span::new(head, "after-landing run lost to a lander restart", Category::Lander, at, resumed).with(PHASE, AFTER)]);
    moved([head], Run::Journaled, "rerun: a stopped lander left it without a verdict");
}

