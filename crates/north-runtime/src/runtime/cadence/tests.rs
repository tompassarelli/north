use super::super::layout::Layout;
use super::super::pin::{self, Event, Reason};
use super::{Heads, Schedule, lines, scheduled};
use std::cell::RefCell;

const PINNED: &str = "1111111111111111111111111111111111111111";
const FIRST: &str = "2222222222222222222222222222222222222222";
const LATE: &str = "3333333333333333333333333333333333333333";
const FIX: &str = "4444444444444444444444444444444444444444";
const AFTER: &str = "5555555555555555555555555555555555555555";
const HOUR: i64 = 3600;
const MIDNIGHT: i64 = 100 * 86_400;
const SLOTS: Schedule = Schedule { spacing: 4 * HOUR, lead: 600 };

#[test]
fn a_landing_between_slots_waits_for_the_slot_and_a_live_fix_promotes_at_once() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_owned() };
    let built = RefCell::new(Vec::<String>::new());
    let pass = |now: i64, pinned: &str, main: &str, fix: Option<&str>| {
        let ready = |sha: &str| built.borrow().iter().any(|ready| ready == sha);
        let mut build = |sha: &str| built.borrow_mut().push(sha.to_owned());
        scheduled(&layout, SLOTS, now, &Heads { pinned, main, fix }, &ready, &mut build)
    };
    let landed = |sha: &str, at: i64| pin::note(&layout, &Event::Landed { sha: sha.to_owned(), at });
    let promoted = |sha: &str, at: i64| pin::note(&layout, &Event::Promoted { sha: sha.to_owned(), at });
    let dues = || pin::read(&layout.state).into_iter().filter(|event| matches!(event, Event::Due { .. })).collect::<Vec<_>>();
    let slot = |sha: &str, at: i64| Event::Due { sha: sha.to_owned(), reason: Reason::Slot, at };

    landed(FIRST, MIDNIGHT + HOUR);
    assert_eq!(pass(MIDNIGHT + HOUR + 20, PINNED, FIRST, None), PINNED, "a landing between slots leaves the pin live");
    assert!(dues().is_empty() && built.borrow().is_empty(), "nothing is due or built between slots");

    assert_eq!(pass(MIDNIGHT + 4 * HOUR - 600, PINNED, FIRST, None), PINNED);
    assert_eq!(pass(MIDNIGHT + 4 * HOUR - 580, PINNED, FIRST, None), PINNED);
    assert_eq!(*built.borrow(), [FIRST], "main's head is built once, ten minutes before the slot");

    landed(LATE, MIDNIGHT + 4 * HOUR - 300);
    assert_eq!(pass(MIDNIGHT + 4 * HOUR + 5, PINNED, LATE, None), FIRST, "the slot promotes the newest head whose runtime is ready, not one that landed after the build");
    assert_eq!(dues(), [slot(FIRST, MIDNIGHT + 4 * HOUR)], "the promotion is due at the slot");
    promoted(FIRST, MIDNIGHT + 4 * HOUR + 40);
    assert_eq!(pass(MIDNIGHT + 4 * HOUR + 60, FIRST, LATE, None), FIRST, "a slot promotes once");

    landed(FIX, MIDNIGHT + 5 * HOUR);
    assert_eq!(pass(MIDNIGHT + 5 * HOUR + 10, FIRST, FIX, Some(FIX)), FIX, "a Live-fix landing is promoted at once");
    let fast = Event::Due { sha: FIX.to_owned(), reason: Reason::FastPath, at: MIDNIGHT + 5 * HOUR + 10 };
    assert_eq!(dues(), [slot(FIRST, MIDNIGHT + 4 * HOUR), fast.clone()], "the fast path is due when its landing is seen");
    promoted(FIX, MIDNIGHT + 5 * HOUR + 70);
    assert_eq!(pass(MIDNIGHT + 5 * HOUR + 200, FIRST, FIX, Some(FIX)), FIRST, "a fix rolled back after its promotion is not promoted again");

    landed(AFTER, MIDNIGHT + 6 * HOUR);
    assert_eq!(pass(MIDNIGHT + 8 * HOUR - 300, FIX, AFTER, None), FIX);
    assert_eq!(pass(MIDNIGHT + 8 * HOUR + 5, FIX, AFTER, None), FIX, "the fast path took the next slot's place");
    assert_eq!((dues().len(), built.borrow().len()), (2, 1), "the slot a fast path took is neither due nor built for");
    assert_eq!(pass(MIDNIGHT + 12 * HOUR - 600, FIX, AFTER, None), FIX);
    assert_eq!(pass(MIDNIGHT + 12 * HOUR + 1, FIX, AFTER, None), AFTER, "the slot after it promotes what waited");
    assert_eq!(dues(), [slot(FIRST, MIDNIGHT + 4 * HOUR), fast, slot(AFTER, MIDNIGHT + 12 * HOUR)]);
    assert_eq!(pass(MIDNIGHT + 12 * HOUR + 21, FIX, AFTER, None), AFTER, "a due promotion not yet live is still promoted on the next pass");

    let events = pin::read(&layout.state);
    let shown = lines(SLOTS, &events, MIDNIGHT + 12 * HOUR + 30);
    assert_eq!(shown, format!("promotions in 24 h: 2\nfast paths in 24 h: 1\nnext slot: 1970-04-11T16:00:00Z (unix {})\n", MIDNIGHT + 16 * HOUR));
    assert_eq!(pin::bars(&events, MIDNIGHT, MIDNIGHT + 12 * HOUR + 30).due_to_live_seconds, [40.0, 60.0, 30.0], "the bar reads due to live, and a landing waiting for its slot is not late");
    assert_eq!(pin::bars(&events, MIDNIGHT, MIDNIGHT + 12 * HOUR).runtime_age_hours, Some(6.0), "the runtime is as old as the wait of the oldest landing no promotion has made live");
}
