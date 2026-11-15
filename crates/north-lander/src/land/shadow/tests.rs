#[test]
fn a_shadow_round_skips_the_queued_candidate_the_lander_is_checking() {
    use north_base::land_ticket::{Phase, Records};
    use super::super::queue::Ticket;
    use super::pick::not_checking;

    let queue = tempfile::tempdir().unwrap();
    let held = queue.path().join("held.json");
    let waiting = queue.path().join("waiting.json");
    let mut records = Records::open(queue.path()).unwrap();
    records.moved(&held, Phase::Queued, "queued").unwrap();
    records.moved(&waiting, Phase::Queued, "queued").unwrap();
    records.checking(std::slice::from_ref(&held)).unwrap();

    let queued = vec![(held, Ticket::default()), (waiting.clone(), Ticket::default())];
    let kept: Vec<_> = not_checking(queued, queue.path()).into_iter().map(|(path, _)| path).collect();
    assert_eq!(kept, vec![waiting]);
}
