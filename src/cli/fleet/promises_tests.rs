use std::cell::RefCell;

use super::*;
use crate::fleet::promises::{landed_as, promised};
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

#[test]
fn a_promise_naming_a_lane_sha_that_landed_rebased_is_judged_against_the_landed_sha() {
    let events = [
        r#"{"pid":7,"sha":"38d2c29e72ff3e63de006a0fa1b88b942a5d77f5","stage":"land checks","status":"wait"}"#,
        r#"{"pid":7,"sha":"bea3cbee02ca8bbdd94b6e9f0c4a1d7f3e2a5b69","stage":"exit","status":"landed"}"#,
        r#"{"pid":8,"sha":"1111111111111111111111111111111111111111","stage":"exit","status":"refused"}"#,
    ]
    .join("\n");
    let lane_sha = "git merge-base --is-ancestor 38d2c29e72ff origin/main";
    assert_eq!(landed_as(lane_sha, &events), "git merge-base --is-ancestor bea3cbee02ca origin/main");
    assert_eq!(landed_as("git cat-file -e bea3cbee02ca", &events), "git cat-file -e bea3cbee02ca");
    assert_eq!(landed_as("git cat-file -e 1111111", &events), "git cat-file -e 1111111", "a lane that did not land keeps its sha");
}

#[test]
fn a_box_line_yields_only_its_done_when_command_not_the_backticks_in_its_prose() {
    let reply = "- [ ] (not before 2026-11-09T13:00Z) The `agent text reads` fall from 74 lines to 10, each `north fleet need` replaced. Done when: `cargo test --test it agent_text_reads` passes.\nLanded by 2026-11-09T13:00Z, see `north fleet list` and `git log -1 origin/main`\n";
    let found = promised(reply);
    assert_eq!(found.iter().map(|(_, command, _)| command.as_str()).collect::<Vec<_>>(), ["cargo test --test it agent_text_reads", "git log -1 origin/main"]);
}
