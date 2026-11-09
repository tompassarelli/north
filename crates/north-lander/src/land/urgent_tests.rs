use super::*;

fn ticket(branch: &str, micros: u128) -> queue::Ticket {
    queue::Ticket { branch: branch.to_owned(), head: format!("{micros:040}"), queued_micros: Some(micros), urgent: Some("queued urgent".to_owned()), ..queue::Ticket::default() }
}

#[test]
fn a_ticket_marked_urgent_again_goes_before_every_urgent_ticket_that_arrived_earlier_and_its_printed_place_matches() {
    let directory = tempfile::tempdir().unwrap();
    let origin = "https://github.com/tompassarelli/north.git";
    let queue = queue::directory(directory.path(), origin);
    let a = queue.join(queue::file_name(1_000, "a"));
    let b = queue.join(queue::file_name(2_000, "b"));
    let reason = "deferred: conflicts with lanes ahead (Cargo.toml, crates/north-base/src/foundation/mod.rs)";
    queue::write(&a, &ticket("a", 1_000)).unwrap();
    queue::write(&b, &ticket("b", 2_000)).unwrap();
    queue::defer(&b, reason).unwrap();

    let said = mark(&queue, "b", "the proxy pays the debt").unwrap();

    let order: Vec<String> = queue::collect(&queue).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["b", "a"], "{said}");
    assert!(said.contains("is 1 of 2 queued"), "{said}");
    let status = super::super::status::report(directory.path(), origin, "b").unwrap();
    assert!(status.contains("is 1 of 2 queued"), "{status}");
    assert!(status.contains(reason), "{status}");
    queue::write(&queue.join(queue::OUTBOX).join(queue::file_name(3_000, "c")), &ticket("c", 3_000)).unwrap();
    let waiting = super::super::status::report(directory.path(), origin, "c").unwrap();
    assert!(waiting.contains("outbox"), "{waiting}");
}
