use super::*;
use crate::{Arrow, Moved};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State2 {
    Idle,
    Held,
    Done,
}

impl State for State2 {
    const ALL: &'static [Self] = &[Self::Idle, Self::Held, Self::Done];

    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Held => "held",
            Self::Done => "done",
        }
    }
}

struct Machine;

impl Lifecycle for Machine {
    const NAME: &'static str = "pair-test";
    type State = State2;

    fn legal(from: Option<State2>, to: State2) -> bool {
        matches!(
            (from, to),
            (None, State2::Idle | State2::Held) | (Some(State2::Idle), State2::Held | State2::Done) | (Some(State2::Held), State2::Idle | State2::Done)
        )
    }

    fn dwell(_: State2) -> Option<std::time::Duration> {
        None
    }

    fn orphan(_: State2) -> Option<State2> {
        None
    }
}

fn admits(_: State2) -> bool {
    true
}

fn one_race() -> usize {
    1
}

fn two_races() -> usize {
    2
}

#[test]
fn a_second_instance_starting_while_the_first_is_held_is_a_broken_promise() {
    let together = Together { alone: &[State2::Held], admits, race: one_race };
    let outcome = explore::<Machine>(&together).unwrap();
    assert!(outcome.findings.iter().any(|finding| finding.rule == "together"), "{:?}", outcome.findings);
    assert!(outcome.moves.contains(&Moved { from: None, to: "held", arrow: Arrow::Writer }));
}

#[test]
fn a_second_instance_starting_while_the_first_is_idle_is_not_a_broken_promise() {
    let together = Together { alone: &[], admits, race: one_race };
    let outcome = explore::<Machine>(&together).unwrap();
    assert!(outcome.findings.is_empty(), "{:?}", outcome.findings);
}

#[test]
fn a_race_the_guard_admits_twice_is_a_broken_promise() {
    let together = Together { alone: &[State2::Held], admits, race: two_races };
    let outcome = explore::<Machine>(&together).unwrap();
    assert!(outcome.findings.iter().any(|finding| finding.text.contains("both hold the name")), "{:?}", outcome.findings);
}
