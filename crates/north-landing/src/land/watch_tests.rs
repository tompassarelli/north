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
}

#[test]
fn a_landing_on_a_base_the_waiting_run_does_not_end_at_waits_as_its_own_run() {
    let mut pending = Pending::default();
    pending.add(batch("a", "b"));
    pending.add(batch("x", "y"));
    assert_eq!(pending.next(0.0).map(|batch| batch.head), Some("b".to_owned()));
    assert_eq!(pending.next(0.0).map(|batch| batch.base), Some("x".to_owned()));
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
}
