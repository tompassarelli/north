use super::super::layout::{self, Layout};
use super::super::lifecycle::{self, Promoting, Promotion};
use super::{Ahead, Bars, Event, Reason, ahead, bars, page, status};
use std::os::unix::fs::symlink;

const PINNED: &str = "1111111111111111111111111111111111111111";
const MAIN: &str = "2222222222222222222222222222222222222222";
const HOUR: i64 = 3600;
const NOW: i64 = 100 * 86_400;

#[test]
fn main_is_ahead_of_the_pin_only_when_a_commit_since_it_changes_a_runtime_input() {
    let unreached = |_: &str| -> bool { panic!("no diff is read without a pin, or with the pin at main") };
    assert_eq!(ahead(Some(PINNED), MAIN, |pinned| pinned == PINNED), Ahead::Changed);
    assert_eq!(ahead(Some(PINNED), MAIN, |_| false), Ahead::Unchanged);
    assert_eq!(ahead(Some(MAIN), MAIN, unreached), Ahead::Current);
    assert_eq!(ahead(None, MAIN, unreached), Ahead::Changed);
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
    assert_eq!(bars(&events, NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: Some(1), rollback_seconds: Some(150.0), rollbacks: 2, due_to_live_seconds: vec![], runtime_age_hours: Some(0.0) });
    assert_eq!(bars(&events[..3], NOW - 24 * HOUR, NOW).breaks_reaching_second_host, Some(0));
    assert_eq!(bars(&events, NOW + HOUR, NOW + 2 * HOUR).breaks_reaching_second_host, Some(0), "a quiet window after a promotion breaks nothing");
    assert_eq!(bars(&[], NOW - 24 * HOUR, NOW), Bars { breaks_reaching_second_host: None, rollback_seconds: None, rollbacks: 0, due_to_live_seconds: vec![], runtime_age_hours: None });
}

#[test]
fn a_due_promotion_not_live_after_5_min_pages_the_coordinator_once() {
    const THIRD: &str = "3333333333333333333333333333333333333333";
    fn deliver(pages: &mut Vec<String>) -> impl FnMut(&str) -> bool + '_ {
        |text| {
            pages.push(text.to_owned());
            true
        }
    }
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let landed = |sha: &str, at: i64| super::note(&layout, &Event::Landed { sha: sha.to_owned(), at });
    let due = |sha: &str, reason: Reason, at: i64| super::note(&layout, &Event::Due { sha: sha.to_owned(), reason, at });
    let promoted = |sha: &str, at: i64| super::note(&layout, &Event::Promoted { sha: sha.to_owned(), at });
    let mut pages = Vec::<String>::new();
    landed(MAIN, NOW - 3 * HOUR);
    page(&layout, NOW, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert!(pages.is_empty(), "a landing waiting for its slot is on schedule");
    due(MAIN, Reason::Slot, NOW);
    page(&layout, NOW + 299, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert!(pages.is_empty(), "a promotion due under five minutes ago waits");
    page(&layout, NOW + 300, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert_eq!(pages.len(), 1, "a due promotion five minutes from live pages the coordinator");
    assert!(pages[0].contains("slot promotion of 22222222") && pages[0].contains("due at 1970-04-11T00:00:00Z") && pages[0].contains("300s"), "{}", pages[0]);
    due(THIRD, Reason::FastPath, NOW + 400);
    page(&layout, NOW + 900, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert_eq!(pages.len(), 1, "a promotion that falls due behind a stuck one joins its page");
    promoted(THIRD, NOW + 1000);
    page(&layout, NOW + 2000, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert_eq!(pages.len(), 1, "a served promotion needs no page");
    let mut refusals = 0;
    due(PINNED, Reason::FastPath, NOW + 2000);
    page(&layout, NOW + 2400, "laptop", &mut |_, _| false, &mut |_| {
        refusals += 1;
        false
    });
    page(&layout, NOW + 2401, "laptop", &mut |_, _| false, &mut deliver(&mut pages));
    assert_eq!((refusals, pages.len()), (1, 2), "a page that was not delivered is sent again");
    assert!(pages[1].contains("Live-fix promotion of 11111111"), "{}", pages[1]);
}

#[test]
fn a_due_promotion_the_host_already_runs_is_not_paged() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    symlink(layout::link(MAIN), layout.current()).unwrap();
    super::note(&layout, &Event::Due { sha: MAIN.to_owned(), reason: Reason::Slot, at: NOW - 300 });
    let pages = std::cell::RefCell::new(Vec::<String>::new());
    let mut send = |text: &str| {
        pages.borrow_mut().push(text.to_owned());
        true
    };
    page(&layout, NOW, "laptop", &mut |_, _| panic!("the live runtime is the due promotion itself"), &mut send);
    assert!(pages.borrow().is_empty(), "a due sha that is the live pin is live, with no promotion journaled");

    super::note(&layout, &Event::Due { sha: PINNED.to_owned(), reason: Reason::FastPath, at: NOW - 600 });
    page(&layout, NOW, "laptop", &mut |live, due| live == MAIN && due == PINNED, &mut send);
    assert!(pages.borrow().is_empty(), "a due sha the live runtime descends from is live");

    page(&layout, NOW, "laptop", &mut |_, _| false, &mut send);
    assert_eq!(pages.borrow().len(), 1, "a due sha the live runtime does not contain still pages");
    assert!(pages.borrow()[0].contains("promotion of 11111111") && pages.borrow()[0].contains("600s") && pages.borrow()[0].contains("pinned runtime is 22222222"), "{}", pages.borrow()[0]);
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
