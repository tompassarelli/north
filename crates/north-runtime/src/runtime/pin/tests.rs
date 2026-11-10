use super::super::canary::Verdict;
use super::super::failure::failed;
use super::super::layout::{self, Layout};
use super::super::live::Swap;
use super::{Bars, Due, Event, PER_DAY, Reason, bars, due, judged, read, ship_if_walked, status};
use std::cell::Cell;
use std::fs;
use std::os::unix::fs::symlink;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";
const HOUR: i64 = 3600;
const NOW: i64 = 100 * 86_400;

#[test]
fn a_landing_waits_for_the_cadence_unless_it_fixes_red_main() {
    assert_eq!(due(Some(PINNED), MAIN, &[NOW - HOUR], NOW, false), Due::Stay { next_at: NOW + 3 * HOUR });
    assert_eq!(due(Some(PINNED), MAIN, &[NOW - HOUR], NOW, true), Due::Promote(Reason::RedMainFix));
    assert_eq!(due(Some(PINNED), MAIN, &[NOW - 5 * HOUR], NOW, false), Due::Promote(Reason::Cadence));
    let full_day: Vec<i64> = (0..PER_DAY as i64).map(|step| NOW - 23 * HOUR + step * 30 * 60).collect();
    assert_eq!(due(Some(PINNED), MAIN, &full_day[..PER_DAY - 1], NOW, false), Due::Promote(Reason::Cadence));
    assert_eq!(due(Some(PINNED), MAIN, &full_day, NOW, false), Due::Stay { next_at: NOW + HOUR });
    assert_eq!(due(Some(MAIN), MAIN, &[], NOW, true), Due::Stay { next_at: 0 });
    assert_eq!(due(None, MAIN, &[NOW], NOW, false), Due::Promote(Reason::First));
}

#[test]
fn the_runtime_bars_count_a_break_only_when_it_reached_a_second_host() {
    let rolled = |host: &str, walker: bool, seconds: i64| Event::RolledBack { sha: MAIN.to_owned(), host: host.to_owned(), at: NOW - HOUR, seconds, walker };
    let events = vec![
        Event::Promoted { sha: PINNED.to_owned(), at: NOW - 30 * HOUR, reason: Reason::Cadence },
        Event::Promoted { sha: MAIN.to_owned(), at: NOW - 2 * HOUR, reason: Reason::Cadence },
        rolled("whiterabbit", true, 70),
        rolled("gex45", false, 150),
    ];
    assert_eq!(bars(&events, NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: Some(1), rollback_seconds: Some(150.0), rollbacks: 2, runtime_age_hours: Some(0.0) });
    assert_eq!(bars(&events[..3], NOW - 24 * HOUR, NOW).breaks_reaching_second_host, Some(0));
    assert_eq!(bars(&events, NOW + HOUR, NOW + 2 * HOUR).breaks_reaching_second_host, Some(0), "a quiet window after a promotion breaks nothing");
    assert_eq!(bars(&[], NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: None, rollback_seconds: None, rollbacks: 0, runtime_age_hours: None });
}

#[test]
fn the_runtime_is_as_old_as_the_wait_since_main_first_moved_past_the_last_promotion() {
    let waiting = |at: i64| Event::Waiting { sha: MAIN.to_owned(), at };
    let promoted = |at: i64| Event::Promoted { sha: MAIN.to_owned(), at, reason: Reason::Cadence };
    let events = vec![promoted(NOW - 40 * HOUR), waiting(NOW - 30 * HOUR), waiting(NOW - 20 * HOUR)];
    assert_eq!(bars(&events, NOW - HOUR, NOW).runtime_age_hours, Some(30.0), "main has waited since its first move past the pin");
    assert_eq!(bars(&events, NOW - 40 * HOUR, NOW - 35 * HOUR).runtime_age_hours, Some(0.0), "the pin was main then");
    let caught_up = [events, vec![promoted(NOW - 2 * HOUR)]].concat();
    assert_eq!(bars(&caught_up, NOW - HOUR, NOW).runtime_age_hours, Some(0.0), "a promotion ends the wait");
}

#[test]
fn a_rollback_is_journaled_with_its_seconds_live_and_whether_the_walking_host_took_it() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let swaps = vec![Swap { runtime: PINNED.to_owned(), at: 10, estimated: false }, Swap { runtime: MAIN.to_owned(), at: super::now() - 90, estimated: false }];
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

#[test]
fn status_names_the_pinned_runtime_when_it_was_promoted_and_whether_main_waits() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    symlink(layout::link(MAIN), layout.current()).unwrap();
    super::note(&layout, &Event::Promoted { sha: MAIN.to_owned(), at: 1_000, reason: Reason::RedMainFix });
    super::note(&layout, &Event::Waiting { sha: PINNED.to_owned(), at: 2_000 });
    let shown = status(&layout.state, layout.live_sha().as_deref());
    assert!(shown.contains(&format!("pinned: {MAIN}")) && shown.contains("promoted at: 1970-01-01T00:16:40Z (unix 1000, RedMainFix)") && shown.contains("main waiting since: 1970-01-01T00:33:20Z (unix 2000)"), "{shown}");
    let adopted = tempfile::tempdir().unwrap();
    let adopted_layout = Layout { state: adopted.path().to_owned() };
    symlink(layout::link(MAIN), adopted_layout.current()).unwrap();
    fs::write(adopted.path().join("runtime-promote.json"), format!(r#"{{"sha":"{MAIN}","state":"live","at":3000}}"#)).unwrap();
    let shown = status(&adopted_layout.state, adopted_layout.live_sha().as_deref());
    assert!(shown.contains("promoted at: 1970-01-01T00:50:00Z (unix 3000, recorded on this host)"), "{shown}");
}
