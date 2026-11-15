use std::collections::BTreeSet;

use north_landing::foundation::check_log::Failure;
use north_landing::land::post_land::reproduced;

use super::{Batch, Pending};

fn batch(base: &str, head: &str) -> Batch {
    Batch::landed(base.to_owned(), head.to_owned(), Vec::new(), 0.0)
}

#[test]
fn a_run_without_a_verdict_absorbs_the_landings_after_it_and_reruns_behind_the_other_waiting_runs_after_its_delay() {
    let mut pending = Pending::default();
    pending.add(batch("b", "c"));
    pending.add(batch("d", "e"));
    let mut failed = batch("a", "b");
    failed.retry_at = 30.0;
    pending.retry(failed);
    let later = pending.next(30.0).expect("the later landing");
    assert_eq!((later.base.as_str(), later.head.as_str()), ("d", "e"), "a landing waiting behind no rerun starts first");
    let mut rerun = pending.next(30.0).expect("the rerun");
    assert_eq!((rerun.base.as_str(), rerun.head.as_str()), ("a", "c"));
    assert!(rerun.covers("b") && rerun.covers("c"));
    assert!(pending.next(30.0).is_none());
    rerun.retry_at = 60.0;
    pending.retry(rerun);
    assert!(pending.next(30.0).is_none(), "a rerun waits out its delay");
    assert!(pending.next(60.0).is_some());
    let case = Failure::Cargo("landing::red_check::tests::one".to_owned());
    let failing = BTreeSet::from([case.clone()]);
    assert!(reproduced(&failing, |_| true, &BTreeSet::new()).is_none(), "a rerun that ran the case and did not fail it reproduces nothing, so no lane is reverted");
    let again = BTreeSet::from(["landing::red_check::tests::one".to_owned()]);
    assert_eq!(reproduced(&failing, |_| true, &again), Some(BTreeSet::from([case])), "a rerun that fails the case reproduces it, so its lane is reverted");
}
