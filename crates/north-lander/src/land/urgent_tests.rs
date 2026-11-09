use super::*;

fn ticket(branch: &str, micros: u128) -> queue::Ticket {
    queue::Ticket { branch: branch.to_owned(), head: format!("{micros:040}"), queued_micros: Some(micros), urgent: Some("queued urgent".to_owned()), ..queue::Ticket::default() }
}

#[test]
fn a_ticket_marked_urgent_again_goes_before_every_urgent_ticket_that_arrived_earlier_and_its_printed_place_matches() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join(queue::file_name(1_000, "a"));
    let b = directory.path().join(queue::file_name(2_000, "b"));
    queue::write(&a, &ticket("a", 1_000)).unwrap();
    queue::write(&b, &ticket("b", 2_000)).unwrap();

    let said = mark(directory.path(), "b", "the proxy pays the debt").unwrap();

    let order: Vec<String> = queue::collect(directory.path()).unwrap().into_iter().map(|(_, ticket)| ticket.branch).collect();
    assert_eq!(order, ["b", "a"], "{said}");
    assert!(said.contains("is 1 of 2 queued"), "{said}");
}
