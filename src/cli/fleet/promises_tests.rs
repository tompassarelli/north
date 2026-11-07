use std::cell::RefCell;

use super::*;
use crate::fleet::promises::promised;
use crate::foundation::clock::Timestamp;

fn unix_ms(text: &str) -> u64 {
    u64::try_from(Timestamp::parse(text).unwrap().unix_millis()).unwrap()
}

#[test]
fn a_report_promising_a_state_by_a_wall_time_is_checked_then_and_its_failing_command_tells_the_proxy_once() {
    let reply = "Not done: the tripwire reading is refreshed.\nLaunches on b open by 2:16 AM: `test -e /nonexistent/n838-launches-open`\nLanded at 1:50 AM: `git merge-base --is-ancestor HEAD origin/main`\nStop: capacity\n";
    let said_ms = unix_ms("2026-11-07T19:00:00Z");
    let promise = |index: usize| {
        let (line, command, when) = promised(reply).swap_remove(index);
        Promise { session: "gex45-lead-b".into(), turn: 4, line, command, when, said_ms, cwd: "/nonexistent".into() }
    };
    assert_eq!(promised(reply).len(), 2);
    let tom = Zone::Fixed(7 * 3600);
    let (launches, landed) = (promise(0), promise(1));
    assert_eq!(due_ms(&launches, &tom), Some(unix_ms("2026-11-07T19:16:00Z")));
    assert_eq!(due_ms(&landed, &tom), None, "a time before the report is a statement of the past, not a promise");
    let told = RefCell::new(Vec::new());
    let line = check(&launches, &|text| {
        told.borrow_mut().push(text.to_owned());
        Ok(())
    });
    assert_eq!(told.borrow().len(), 1);
    assert!(told.borrow()[0].contains("promise not kept: gex45-lead-b") && told.borrow()[0].contains("`test -e /nonexistent/n838-launches-open` exited 1"), "{}", told.borrow()[0]);
    assert!(line.ends_with("(proxy told)"), "{line}");
}
