use super::{Answer, Estimate, Judged, Spending, State, judge};

const ISSUE: &str = "north#385";

fn worker(name: &str, spent: f64, created_ms: u64, p90: f64) -> Spending {
    Spending {
        name: name.to_owned(),
        parent: Some("lead-385".to_owned()),
        workstream: ISSUE.to_owned(),
        box_id: format!("{ISSUE}@/lane"),
        lead: false,
        tom: false,
        live: true,
        spent: Some(spent),
        minutes: 30.0,
        created_ms,
        estimate: Some(Estimate { p50: Some(p90 / 2.0), p90, minutes: 30.0 }),
    }
}

fn tick(now_ms: u64, sessions: &[Spending], answers: &[Answer], state: &State) -> Judged {
    judge(now_ms, sessions, answers, state)
}

fn kinds(judged: &Judged) -> Vec<(&'static str, String)> {
    judged.events.iter().map(|event| (event.kind, event.scope.clone())).collect()
}

/// North#385 ran up $1.86 against a p90 of $0.37, 5.0x.
fn replay() -> (Judged, Judged, Judged) {
    let early = tick(1_000, &[worker("n385", 0.20, 0, 0.37)], &[], &State::default());
    let first = tick(2_000, &[worker("n385", 1.86, 0, 0.37)], &[], &early.state);
    let second = tick(3_000, &[worker("n385", 1.90, 0, 0.37)], &[], &first.state);
    (early, first, second)
}

#[test]
fn a_run_past_four_times_p90_trips_on_the_second_tick_and_not_the_first() {
    let (early, first, second) = replay();
    assert!(early.events.is_empty() && early.tells.is_empty());
    assert!(first.events.iter().all(|event| event.kind != "trip") && first.tells.is_empty() && first.alerts.is_empty(), "{first:?}");
    assert!(kinds(&second).contains(&("trip", format!("workstream:{ISSUE}"))), "{second:?}");
    assert_eq!(second.alerts.len(), 1, "Tom sees the trip once");
}

#[test]
fn a_tripped_session_is_told_once_and_a_lead_is_not() {
    let (_, _, second) = replay();
    let lead = Spending { name: "lead-385".into(), parent: None, lead: true, ..worker("lead-385", 5.0, 0, 0.37) };
    let third = tick(4_000, &[worker("n385", 2.0, 0, 0.37), lead.clone()], &[], &second.state);
    let fourth = tick(5_000, &[worker("n385", 2.1, 0, 0.37), lead], &[], &third.state);
    assert_eq!(second.tells.iter().map(|tell| tell.to.as_str()).collect::<Vec<_>>(), ["n385"]);
    assert!(second.tells[0].text.contains("Stop: budget") && second.tells[0].text.contains("Split:"));
    assert!(third.tells.is_empty() && fourth.tells.is_empty(), "the second and third ticks tell nobody again");
}

#[test]
fn a_newer_brief_whose_budget_states_a_higher_p90_clears_the_trip() {
    let (_, _, second) = replay();
    let renewed = [worker("n385", 1.95, 0, 0.37), worker("n385-b", 0.0, 10_000, 0.60)];
    let cleared = tick(11_000, &renewed, &[], &second.state);
    assert!(kinds(&cleared).contains(&("clear", format!("workstream:{ISSUE}"))), "{cleared:?}");
    assert_eq!(cleared.events.iter().find(|event| event.kind == "clear").and_then(|event| event.by.as_deref()), Some("the brief of n385-b"));
    assert!(cleared.state.tripped(ISSUE).is_none() && !cleared.state.told.contains(ISSUE));
    let later = tick(12_000, &renewed, &[], &cleared.state);
    assert!(later.events.iter().all(|event| event.kind != "trip") && later.tells.is_empty());
}

#[test]
fn a_brief_older_than_the_trip_or_with_no_higher_p90_clears_nothing() {
    let (_, _, second) = replay();
    let same = tick(11_000, &[worker("n385", 1.95, 0, 0.37), worker("n385-b", 0.0, 10_000, 0.37)], &[], &second.state);
    let older = tick(11_000, &[worker("n385", 1.95, 0, 0.37), worker("n385-b", 0.0, 1_000, 0.60)], &[], &second.state);
    assert!(same.events.iter().all(|event| event.kind != "clear") && older.events.iter().all(|event| event.kind != "clear"), "{same:?} {older:?}");
}

#[test]
fn tom_answering_a_gate_with_a_higher_p90_clears_the_trip() {
    let (_, _, second) = replay();
    let answer = Answer { workstream: ISSUE.to_owned(), at_ms: 9_000, by: "tom".into(), text: "Raise it: E p90 $1.00".into() };
    let cleared = tick(10_000, &[worker("n385", 1.95, 0, 0.37)], &[answer], &second.state);
    assert_eq!(cleared.events.iter().find(|event| event.kind == "clear").and_then(|event| event.by.as_deref()), Some("tom's answer to a gate"), "{cleared:?}");
}

#[test]
fn a_warning_waits_for_two_ticks_and_tells_the_lead_while_toms_sessions_are_never_judged() {
    let mut tom = worker("tom", 50.0, 0, 0.37);
    tom.tom = true;
    let first = tick(1_000, &[worker("n385", 0.9, 0, 0.37), tom.clone()], &[], &State::default());
    let second = tick(2_000, &[worker("n385", 0.9, 0, 0.37), tom], &[], &first.state);
    assert!(first.events.is_empty());
    assert_eq!(kinds(&second).into_iter().filter(|(kind, _)| *kind == "warn").count(), 2, "the box and its workstream");
    assert_eq!(second.tells.iter().map(|tell| tell.to.as_str()).collect::<Vec<_>>(), ["lead-385"]);
}

#[test]
fn a_box_with_no_stated_budget_is_never_tripped_and_a_finished_box_leaves_an_outcome() {
    let bare = Spending { estimate: None, ..worker("n385", 99.0, 0, 0.37) };
    let one = tick(1_000, std::slice::from_ref(&bare), &[], &State::default());
    let two = tick(2_000, std::slice::from_ref(&bare), &[], &one.state);
    assert!(two.events.is_empty() && two.tells.is_empty());
    let (_, _, tripped) = replay();
    let finished = tick(9_000, &[], &[], &tripped.state);
    let outcome = finished.events.iter().find(|event| event.kind == "outcome" && event.scope == format!("workstream:{ISSUE}")).expect("an outcome");
    assert_eq!((outcome.spent, outcome.by.as_deref()), (1.90, Some("tripped")));
    assert!(finished.state.scopes.is_empty() && finished.state.told.is_empty());
}

#[test]
fn the_state_survives_a_write_and_a_read() {
    let (_, _, second) = replay();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("breaker.json");
    assert_eq!(State::read(&path).unwrap(), State::default());
    second.state.write(&path).unwrap();
    assert_eq!(State::read(&path).unwrap(), second.state);
}
