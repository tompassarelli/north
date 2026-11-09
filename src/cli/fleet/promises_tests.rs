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
        let claim = promised(reply, &|_| true).swap_remove(index);
        Promise { session: "gex45-lead-b".into(), turn: 4, line: claim.line, command: claim.command.unwrap(), when: claim.when, said_ms, cwd: "/nonexistent".into() }
    };
    assert_eq!(promised(reply, &|_| true).len(), 2);
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
fn a_promised_command_that_fails_on_its_own_usage_reaches_no_verdict_and_tells_the_proxy_nothing() {
    let line = "- **The 6-hour box:** it can't read 0 until about 10:19 AM, because the last errors were at 4:19 AM. As you directed, I removed that wait-only read and rewrote the box as a check that runs now. `north task check --done-when` ran it and ticked the box: `pass (exit 0)`.";
    let usage = "echo 'error: north task check --done-when [--dry-run] [--main DIR] [--box TEXT] (--body-file FILE | REPO#N | --repo OWNER/REPO)' >&2; exit 2";
    let promise = Promise { session: "bd-845-nullacct".into(), turn: 8, line: line.into(), command: usage.into(), when: When::Wall { hour: 10, minute: 19, date: None }, said_ms: 1_794_086_580_737, cwd: "/nonexistent".into() };
    let told = RefCell::new(0);
    let verdict = check(&promise, &|_| {
        *told.borrow_mut() += 1;
        Ok(())
    });
    assert_eq!(*told.borrow(), 0, "{verdict}");
    assert!(verdict.starts_with("promise not judged: bd-845-nullacct"), "{verdict}");
}

#[test]
fn a_promise_naming_a_date_is_due_on_that_date_not_on_the_nearest_day_with_that_time() {
    let line = "Check after Tue Nov 10, 3:00 PM (when account d's week resets): `north fleet accounts --plan`";
    let claim = promised(line, &|_| true).remove(0);
    assert_eq!(claim.when, When::Wall { hour: 15, minute: 0, date: Some((11, 10)) });
    let promise = Promise { session: "staffing-rules".into(), turn: 3, line: line.into(), command: "true".into(), when: claim.when, said_ms: unix_ms("2026-11-09T03:00:00Z"), cwd: "/".into() };
    let tom = Zone::named(Some("Etc/GMT-7"));
    assert_eq!(due_ms(&promise, &tom), Some(unix_ms("2026-11-10T08:00:00Z")));
    let legacy: Promise = serde_json::from_str(&serde_json::to_string(&promise).unwrap().replace(r#","date":[11,10]"#, "")).unwrap();
    assert_eq!(legacy.when, When::Wall { hour: 15, minute: 0, date: None });
    assert_eq!(legacy.dated().when, promise.when);
}
