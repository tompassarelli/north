use std::collections::BTreeMap;

use north_graph::Graph;

use crate::{CheckDeadline, Held, Remembered, check_deadline_from, remembered_from, tell_check_deadline_to, tell_remembered_to, ticks_from};

#[test]
fn a_pass_tells_the_lander_one_deadline_per_check_and_the_land_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let first = CheckDeadline { seconds: 300, passes: 6, p90: Some(41), exceeded: None };
    tell_check_deadline_to(&mut graph, "local:cargo test", &first, 1_000).unwrap();
    tell_check_deadline_to(&mut graph, "gex45:cargo test", &first, 1_500).unwrap();
    let raised = CheckDeadline { seconds: 1_800, passes: 7, p90: Some(41), exceeded: Some(900) };
    tell_check_deadline_to(&mut graph, "local:cargo test", &raised, 2_000).unwrap();
    assert_eq!((check_deadline_from(&graph, "local:cargo test").unwrap(), check_deadline_from(&graph, "gex45:cargo test").unwrap(), check_deadline_from(&graph, "local:cargo nextest").unwrap()), (Some(raised), Some(first), None));
    assert_eq!(ticks_from(&graph).unwrap().get("land"), Some(&2_000));
}

#[test]
fn a_doctor_round_leaves_only_the_bounds_it_still_holds_and_its_tick() {
    let mut graph = Graph::open_in_memory().unwrap();
    let held = Held { since: 100, remedies: vec![(100, "lander-wedged".to_owned()), (400, "slot-overdue".to_owned())], paged: Some(500) };
    let first = Remembered {
        good: vec!["runtimes/a".to_owned(), "runtimes/b".to_owned()],
        episodes: BTreeMap::from([("round-silence:north".to_owned(), held)]),
        states: BTreeMap::from([("round-silence:north".to_owned(), "over".to_owned()), ("link-lag:north".to_owned(), "ok".to_owned())]),
        incidents: BTreeMap::from([("lander-wedged".to_owned(), 2)]),
        at: Some(600),
    };
    tell_remembered_to(&mut graph, &first, 600).unwrap();
    assert_eq!(remembered_from(&graph).unwrap(), first);
    let second = Remembered { good: vec!["runtimes/b".to_owned()], states: BTreeMap::from([("link-lag:north".to_owned(), "ok".to_owned())]), incidents: first.incidents.clone(), at: Some(720), ..Remembered::default() };
    tell_remembered_to(&mut graph, &second, 720).unwrap();
    assert_eq!(remembered_from(&graph).unwrap(), second);
}
