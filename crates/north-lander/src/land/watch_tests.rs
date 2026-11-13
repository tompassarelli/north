use std::collections::BTreeSet;

use north_landing::foundation::check_log::Failure;
use north_landing::land::post_land::reproduced;

use super::{Batch, Pending};

fn batch(base: &str, head: &str) -> Batch {
    Batch::landed(base.to_owned(), head.to_owned(), Vec::new(), 0.0)
}

#[test]
fn landings_that_follow_each_other_on_main_wait_as_one_run_of_the_newest_head() {
    let mut pending = Pending::default();
    pending.add(batch("a", "b"));
    pending.add(batch("b", "c"));
    pending.add(batch("c", "d"));
    let next = pending.next(0.0).expect("one batch");
    assert_eq!((next.base.as_str(), next.head.as_str()), ("a", "d"));
    assert!(["b", "c", "d"].iter().all(|head| next.covers(head)));
    assert!(pending.next(0.0).is_none());

    let mut separate = Pending::default();
    separate.add(batch("a", "b"));
    separate.add(batch("x", "y"));
    assert_eq!(separate.next(0.0).map(|batch| batch.head), Some("b".to_owned()));
    assert_eq!(separate.next(0.0).map(|batch| batch.base), Some("x".to_owned()), "a landing on a base the waiting run does not end at waits as its own run");
}

#[test]
fn a_run_without_a_verdict_waits_again_absorbing_later_landings_and_reruns_after_its_delay() {
    let mut pending = Pending::default();
    pending.add(batch("b", "c"));
    let mut failed = batch("a", "b");
    failed.retry_at = 30.0;
    pending.retry(failed);
    assert!(pending.next(10.0).is_none(), "no rerun before the delay");
    let rerun = pending.next(30.0).expect("the rerun");
    assert_eq!((rerun.base.as_str(), rerun.head.as_str()), ("a", "c"));
    assert!(rerun.covers("b") && rerun.covers("c"));
    assert!(pending.next(30.0).is_none());
    let case = Failure::Cargo("landing::red_check::tests::one".to_owned());
    let failing = BTreeSet::from([case.clone()]);
    assert!(reproduced(&failing, |_| true, &BTreeSet::new()).is_none(), "a rerun that ran the case and did not fail it reproduces nothing, so no lane is reverted");
    let again = BTreeSet::from(["landing::red_check::tests::one".to_owned()]);
    assert_eq!(reproduced(&failing, |_| true, &again), Some(BTreeSet::from([case])), "a rerun that fails the case reproduces it, so its lane is reverted");
}

#[test]
fn a_run_waiting_out_its_rerun_delay_does_not_hold_back_a_later_run_that_is_ready() {
    let mut pending = Pending::default();
    pending.add(batch("c", "d"));
    let mut failed = batch("a", "b");
    failed.retry_at = 30.0;
    pending.retry(failed);
    assert_eq!(pending.next(10.0).map(|batch| batch.head), Some("d".to_owned()));
    assert!(pending.next(10.0).is_none(), "no rerun before the delay");
    assert_eq!(pending.next(30.0).map(|batch| batch.head), Some("b".to_owned()));
}
