use std::cell::RefCell;
use std::collections::BTreeMap;

use north_threads::threads::attribution::{Harness, Loss, Reading};
use north_unblocks::review::{Owners, classes, refusal};
use north_unblocks::{IssueName, Issues};

use super::review;

struct Tracker;

impl Issues for Tracker {
    fn owned_open(&self, _: &IssueName) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn hourly_class_review_wakes_the_coordinator_once_an_hour_and_holds_its_turn_while_a_repeated_class_has_no_owner() {
    let scratch = tempfile::tempdir().unwrap();
    let owners = Owners { listed: BTreeMap::from([("placement".to_owned(), "north#706".to_owned())]), recorded: scratch.path().join("class-owners.jsonl") };
    let reading = Reading { losses: vec![Loss { harness: Harness::Placement, runs: 3, usd: 2.0 }, Loss { harness: Harness::LaunchRefusal, runs: 2, usd: 1.0 }, Loss { harness: Harness::Halted, runs: 1, usd: 0.5 }], coordinator: (0, 0.0), unclaimed_usd: 0.0, total_usd: 3.5 };
    let open = |_: &IssueName| false;
    let found = classes(&reading, &owners, &open).unwrap();

    let held = refusal(&found).expect("an unowned class seen twice holds the turn");
    assert!(held.contains("launch refusal 2 runs") && !held.contains("placement") && !held.contains("halted"), "{held}");

    let (told, sent) = (scratch.path().join("told"), RefCell::new(Vec::new()));
    let send = |text: &str| {
        sent.borrow_mut().push(text.to_owned());
        Ok(())
    };
    review(&told, 10_000, || Ok(found.clone()), send);
    review(&told, 10_000 + 3_599, || Ok(found.clone()), send);
    assert_eq!(sent.borrow().len(), 1, "the review woke the coordinator twice within the hour");
    assert!(sent.borrow()[0].contains("placement 3 runs $2.00, owner north#706") && sent.borrow()[0].contains("launch refusal 2 runs $1.00, owner none"), "{}", sent.borrow()[0]);
    review(&told, 10_000 + 3_600, || Ok(found.clone()), send);
    assert_eq!(sent.borrow().len(), 2, "the next hour's review did not come");

    assert!(owners.record("no such class", "north#1577", 10_000, &Tracker).is_err());
    owners.record("launch refusal", "north#1577", 10_000, &Tracker).unwrap();
    assert_eq!(refusal(&classes(&reading, &owners, &open).unwrap()), None);

    let closed = |issue: &IssueName| issue.number == 706;
    assert!(refusal(&classes(&reading, &owners, &closed).unwrap()).is_some_and(|held| held.contains("placement 3 runs")), "a closed owner still owned its class");
}
