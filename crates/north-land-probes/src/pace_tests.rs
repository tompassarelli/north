use super::{RefusalRate, paced};
use crate::settle;

#[test]
fn two_refusals_in_the_last_hour_build_no_candidate_over_two_lanes_on_a_queue_of_fourteen() {
    let hour = [false, false, true, false, false, false, true, false];
    let rate = hour.iter().fold(RefusalRate::default(), |rate, refused| rate.observe(*refused));
    let queue: Vec<u32> = (1..=14).collect();
    let probes = paced(&queue, &[], 6, rate, &[]);
    assert!(!probes.is_empty());
    assert!(probes.iter().all(|probe| probe.len() <= 2), "planned {probes:?}");
    assert_eq!(paced(&queue, &[], 6, RefusalRate::default(), &[]).last().map(Vec::len), Some(14));
}

#[test]
fn an_urgent_ticket_lands_in_round_one_when_every_other_ticket_is_bad() {
    let queue: Vec<u32> = (1..=14).collect();
    let probes = paced(&queue, &[9], 6, RefusalRate::default(), &[]);
    assert!(probes.len() <= 6);
    let green: Vec<bool> = probes.iter().map(|probe| probe.iter().all(|lane| *lane == 9)).collect();
    assert_eq!(settle(&probes, &green).land, [9]);
}
