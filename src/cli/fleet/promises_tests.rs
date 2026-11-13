use std::cell::RefCell;

use super::*;
use crate::fleet::promises::{Unreckoned, promised};
use crate::foundation::clock::Timestamp;

fn unix_ms(text: &str) -> u64 {
    u64::try_from(Timestamp::parse(text).unwrap().unix_millis()).unwrap()
}

#[test]
fn a_due_promise_is_a_message_to_the_reporting_sessions_parent_and_its_quoted_command_is_never_started() {
    let scratch = tempfile::tempdir().unwrap();
    let marker = scratch.path().join("started");
    let reply = format!("Not done: the spend reading is refreshed.\nLaunches on b open by 2:16 AM: `touch {}`\nLanded at 1:50 AM: `git merge-base --is-ancestor HEAD origin/main`\nStop: capacity\n", marker.display());
    let said_ms = unix_ms("2026-11-07T19:00:00Z");
    let kept: Vec<Promise> = promised(&reply, said_ms, &Unreckoned, &|_| true)
        .into_iter()
        .map(|claim| Promise { session: "gex45-lead-b".into(), turn: 4, line: claim.line, command: claim.command.unwrap(), when: claim.when, said_ms, parent: "night-lead".into() })
        .collect();
    let book = Book::at(scratch.path());
    book.keep(&kept).unwrap();
    let open = book.open();
    assert_eq!(open.len(), 2);
    let tom = Zone::Fixed(7 * 3600);
    let (launches, landed) = (&open[0], &open[1]);
    assert_eq!(due_ms(launches, &tom), Some(unix_ms("2026-11-07T19:16:00Z")));
    assert_eq!(due_ms(landed, &tom), None, "a time before the report is a statement of the past, not a promise");
    let told = RefCell::new(Vec::new());
    let line = remind(launches, unix_ms("2026-11-07T19:16:00Z"), &|parent, text| {
        told.borrow_mut().push((parent.to_owned(), text.to_owned()));
        Ok(())
    });
    let told = told.into_inner();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].0, "night-lead");
    assert!(told[0].1.starts_with("promise due: gex45-lead-b") && told[0].1.contains("2026-11-07T19:16Z") && told[0].1.contains(&format!("`touch {}`", marker.display())), "{}", told[0].1);
    assert!(line.ends_with("(night-lead told)"), "{line}");
    assert!(!marker.exists(), "the quoted command was started");
}

#[test]
fn a_promise_naming_a_date_is_due_on_that_date_not_on_the_nearest_day_with_that_time() {
    let line = "Check after Tue Nov 10, 3:00 PM (when account d's week resets): `north fleet accounts --plan`";
    let claim = promised(line, 0, &Unreckoned, &|_| true).remove(0);
    assert_eq!(claim.when, When::Wall { hour: 15, minute: 0, date: Some((11, 10)) });
    let promise = Promise { session: "staffing-rules".into(), turn: 3, line: line.into(), command: "true".into(), when: claim.when, said_ms: unix_ms("2026-11-09T03:00:00Z"), parent: "proxy".into() };
    let tom = Zone::named(Some("Etc/GMT-7"));
    assert_eq!(due_ms(&promise, &tom), Some(unix_ms("2026-11-10T08:00:00Z")));
    let legacy: Promise = serde_json::from_str(&serde_json::to_string(&promise).unwrap().replace(r#","date":[11,10]"#, "")).unwrap();
    assert_eq!(legacy.when, When::Wall { hour: 15, minute: 0, date: None });
    assert_eq!(legacy.dated().when, promise.when);
}
