use super::*;

const EVENTS: &str = include_str!("../fixtures/cloud/handback-events.jsonl");
const MBOX: &str = include_str!("../fixtures/cloud/handback.mbox");

fn events() -> Vec<Value> {
    EVENTS
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn without_part(index: usize) -> Vec<Value> {
    let marker = format!("BEGIN-HANDBACK part {index}/3");
    events()
        .into_iter()
        .filter(|event| !event.to_string().contains(&marker))
        .collect()
}

#[test]
fn collect_round_trips_the_latest_multi_part_handback_and_ignores_a_stale_one() {
    assert_eq!(collect(&events()).unwrap(), MBOX.as_bytes());
}

#[test]
fn collect_refuses_a_handback_missing_any_one_part() {
    for index in 1..=3 {
        let error = collect(&without_part(index)).unwrap_err();
        assert!(
            error.contains(&format!("part {index} of 3 is missing")),
            "{error}"
        );
    }
}
