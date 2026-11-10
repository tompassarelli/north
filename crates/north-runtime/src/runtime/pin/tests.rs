use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Promoting, Promotion};
use super::{Bars, Due, Event, bars, due, status};
use std::os::unix::fs::symlink;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";
const HOUR: i64 = 3600;
const NOW: i64 = 100 * 86_400;

#[test]
fn a_landing_is_promoted_at_once_unless_it_changes_no_runtime_input_since_the_pin() {
    let unreached = |_: &str| -> bool { panic!("no diff is read without a pin, or with the pin at main") };
    assert_eq!(due(Some(PINNED), MAIN, |pinned| pinned == PINNED), Due::Promote);
    assert_eq!(due(Some(PINNED), MAIN, |_| false), Due::Unchanged);
    assert_eq!(due(Some(MAIN), MAIN, unreached), Due::Current);
    assert_eq!(due(None, MAIN, unreached), Due::Promote);
}

#[test]
fn the_runtime_bars_count_a_break_only_when_it_reached_a_second_host() {
    let rolled = |host: &str, walker: bool, seconds: i64| Event::RolledBack { sha: MAIN.to_owned(), host: host.to_owned(), at: NOW - HOUR, seconds, walker };
    let events = vec![
        Event::Promoted { sha: PINNED.to_owned(), at: NOW - 30 * HOUR },
        Event::Promoted { sha: MAIN.to_owned(), at: NOW - 2 * HOUR },
        rolled("whiterabbit", true, 70),
        rolled("gex45", false, 150),
    ];
    assert_eq!(bars(&events, NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: Some(1), rollback_seconds: Some(150.0), rollbacks: 2, landing_to_live_seconds: vec![] });
    assert_eq!(bars(&events[..3], NOW - 24 * HOUR, NOW).breaks_reaching_second_host, Some(0));
    assert_eq!(bars(&events, NOW + HOUR, NOW + 2 * HOUR).breaks_reaching_second_host, Some(0), "a quiet window after a promotion breaks nothing");
    assert_eq!(bars(&[], NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: None, rollback_seconds: None, rollbacks: 0, landing_to_live_seconds: vec![] });
}

#[test]
fn a_landing_is_live_at_the_first_promotion_after_it_or_still_waiting_at_the_window_end() {
    let landed = |at: i64| Event::Landed { sha: MAIN.to_owned(), at };
    let promoted = |at: i64| Event::Promoted { sha: MAIN.to_owned(), at };
    let events = vec![landed(NOW - 600), landed(NOW - 300), promoted(NOW - 200), landed(NOW - 100)];
    assert_eq!(bars(&events, NOW - HOUR, NOW).landing_to_live_seconds, [400.0, 100.0, 100.0], "two landings during one build go live together, the newest still waits");
    assert_eq!(bars(&events, NOW - HOUR, NOW - 250).landing_to_live_seconds, [350.0, 50.0], "a promotion after the window's end does not count yet");
}

#[test]
fn status_names_the_pinned_runtime_when_it_was_promoted_and_since_when_landings_wait() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    symlink(layout::link(MAIN), layout.current()).unwrap();
    super::note(&layout, &Event::Promoted { sha: MAIN.to_owned(), at: 1_000 });
    super::note(&layout, &Event::Landed { sha: PINNED.to_owned(), at: 2_000 });
    let shown = status(&layout.state, layout.live_sha().as_deref());
    assert!(shown.contains(&format!("pinned: {MAIN}")) && shown.contains("promoted at: 1970-01-01T00:16:40Z (unix 1000)") && shown.contains("landings waiting since: 1970-01-01T00:33:20Z (unix 2000)"), "{shown}");
    let adopted = tempfile::tempdir().unwrap();
    let adopted_layout = Layout { state: adopted.path().to_owned() };
    symlink(layout::link(MAIN), adopted_layout.current()).unwrap();
    lifecycle::move_to::<Promoting>(&adopted_layout, MAIN, Promotion::Live, "went live before the pin journal existed").unwrap();
    let shown = status(&adopted_layout.state, adopted_layout.live_sha().as_deref());
    assert!(shown.contains("recorded on this host)"), "{shown}");
}
