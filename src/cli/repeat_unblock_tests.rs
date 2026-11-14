//! The coordinator's second hand clear of one failing test within a day is refused unless it names an open, owned class-fix issue (north#1577).

use north_runtime::runtime::trip::{Trip, Trips};
use north_runtime::runtime::trip_command::clear;
use north_unblocks::{Admission, IssueName, Issues, Ledger};

struct Tracker;

impl Issues for Tracker {
    fn owned_open(&self, issue: &IssueName) -> Result<(), String> {
        if issue.number == 1577 { Ok(()) } else { Err(format!("{} is not open", issue.label())) }
    }
}

fn trip(trips: &Trips, head: &str) {
    trips.write(&Trip { head: head.to_owned(), run: Some("19000000001".to_owned()), cases: vec![("north-runtime".to_owned(), "worker_sandbox::confined".to_owned())] }).unwrap();
}

#[test]
fn repeat_unblock_a_second_trip_clear_of_one_failing_test_names_an_open_owned_issue() {
    let scratch = tempfile::tempdir().unwrap();
    let (trips, ledger) = (Trips(scratch.path().join("trips")), Ledger(scratch.path().join("unblocks.jsonl")));
    let at = |now| Admission { ledger: &ledger, issues: &Tracker, now };
    let (first, second, third) = ("a".repeat(40), "b".repeat(40), "c".repeat(40));
    for head in [&first, &second, &third] {
        trip(&trips, head);
    }

    clear(&trips, &at(1_000), &first, "proxy", "the probe flaked", None).unwrap();
    let refused = clear(&trips, &at(1_000 + 3_600), &second, "proxy", "the probe flaked again", None).unwrap_err();
    assert!(refused.contains("second trip clear") && refused.contains("worker_sandbox::confined in north-runtime") && refused.contains("--issue REPO#N"), "{refused}");
    assert!(!trips.cleared(&second).unwrap(), "a refused clear cleared the trip");

    assert!(clear(&trips, &at(1_000 + 3_600), &second, "proxy", "the probe flaked again", Some("north#1")).unwrap_err().contains("north#1 is not open"));
    clear(&trips, &at(1_000 + 3_600), &second, "proxy", "the probe flaked again", Some("north#1577")).unwrap();
    assert!(trips.cleared(&second).unwrap());

    clear(&trips, &at(1_000 + 3_600 + 86_400), &third, "proxy", "a day later", None).unwrap();
}
