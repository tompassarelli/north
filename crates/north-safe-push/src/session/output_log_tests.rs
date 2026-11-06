use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use north_base::foundation::step::Refused;

use super::claim;
use crate::shell::recorded::{self, State};
use crate::shell::settings::Settings;

fn claim_with(state: State, claimed: &str) -> (Result<String, Refused>, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(state));
    let mut settings = Settings::from_env();
    settings.output_claimed = claimed.to_owned();
    (
        claim(&recorded::shell(&state, settings)).map(|claims| claims.list),
        state,
    )
}

fn paths(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(PathBuf::from).collect()
}

#[test]
fn the_claimed_list_names_each_new_log_once_after_the_inherited_ones_and_a_held_or_unopenable_log_refuses()
 {
    let (list, state) = claim_with(
        State {
            logs: paths(&["/log/a", "/log/a", "/log/b"]),
            ..State::default()
        },
        " /log/earlier",
    );
    assert_eq!(list, Ok(" /log/earlier /log/a /log/b".to_owned()));
    assert_eq!(state.borrow().locked, paths(&["/log/a", "/log/b"]));

    let (list, state) = claim_with(
        State {
            logs: paths(&["/log/a"]),
            ..State::default()
        },
        " /log/a",
    );
    assert_eq!(list, Ok(" /log/a".to_owned()));
    assert!(state.borrow().locked.is_empty());

    let busy = State {
        logs: paths(&["/log/a", "/log/b"]),
        busy: paths(&["/log/b"]),
        ..State::default()
    };
    assert_eq!(claim_with(busy, "").0, Err(Refused(1)));
    let unopenable = State {
        logs: paths(&["/log/a"]),
        unavailable: paths(&["/log/a"]),
        ..State::default()
    };
    assert_eq!(claim_with(unopenable, "").0, Err(Refused(1)));
}
