use super::super::canary::Verdict;
use super::super::failure::failed;
use super::super::layout::Layout;
use super::super::live::Swap;
use super::super::pin::{Event, read};
use super::{judged, ship_if_walked};
use std::cell::Cell;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";

#[test]
fn a_rollback_is_journaled_with_its_seconds_live_and_whether_the_walking_host_took_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let swaps = vec![Swap { runtime: PINNED.to_owned(), at: 10, estimated: false }, Swap { runtime: MAIN.to_owned(), at: super::super::pin::now() - 90, estimated: false }];
    judged(&layout, "tom@gex45", MAIN, &Verdict::RolledBack, &swaps);
    judged(&layout, "tom@gex45", MAIN, &Verdict::Passed, &swaps);
    judged(&layout, &crate::platform::host::name(), MAIN, &Verdict::Passed, &swaps);
    let events = read(&layout.state);
    assert_eq!(events.len(), 2, "a pass on another host is no walk: {events:?}");
    match &events[0] {
        Event::RolledBack { host, seconds, walker, .. } => assert!(host == "tom@gex45" && !*walker && (90..95).contains(seconds), "{events:?}"),
        other => panic!("not a rollback: {other:?}"),
    }
    assert!(matches!(&events[1], Event::Walked { sha, .. } if sha == MAIN));
}

#[test]
fn nothing_ships_to_another_host_until_the_runtime_walked_the_spine_here() {
    let shipped = Cell::new(0);
    let ship = |_: &str| {
        shipped.set(shipped.get() + 1);
        Ok(())
    };
    assert_eq!(ship_if_walked(Err("rolled back".to_owned()), ship).unwrap(), None);
    assert_eq!(shipped.get(), 0);
    assert_eq!(ship_if_walked(Ok(MAIN.to_owned()), ship).unwrap(), Some(()));
    assert_eq!(shipped.get(), 1);
    assert!(ship_if_walked(Ok(MAIN.to_owned()), |_| Err::<(), _>(failed(1, "unreachable"))).is_err());
}
