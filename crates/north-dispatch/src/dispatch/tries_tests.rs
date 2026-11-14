use super::fleets;
use super::issues::IssueRef;
use super::record::{self, Record};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn launch(item: &str, name: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None, route: None }
}

#[test]
fn a_laptop_launch_absent_from_a_copy_older_than_it_still_holds_its_item() {
    let now = 100_000.0;
    let on_laptop = |item: &str, name: &str, at: f64| Record { host: "whiterabbit".into(), ..launch(item, name, at) };
    let records = [on_laptop("north#293", "north-293-d1300", now - 40.0 * 60.0), on_laptop("north#294", "north-294-d1200", now - 90.0 * 60.0)];
    let listings = vec![("whiterabbit".to_owned(), json!([{"name": "tom-notes", "status": "idle"}]))];
    let copies = BTreeMap::from([("whiterabbit".to_owned(), now - 60.0 * 60.0)]);
    let gone = fleets::gone(&listings, &copies, &BTreeSet::new(), &records, now);
    let holds = record::holds(&records, now, &BTreeMap::new(), &gone);
    let launching = record::launching(&records, now, &gone);
    let after_copy = IssueRef::new("north", 293);
    assert!(holds.contains_key(&after_copy) && launching.contains_key(&after_copy), "a copy taken before the launch cannot show its session forgotten: {gone:?}");
    let before_copy = IssueRef::new("north", 294);
    assert!(!holds.contains_key(&before_copy) && !launching.contains_key(&before_copy), "a copy taken well after the launch that lacks its session shows it forgotten: {gone:?}");
    let fresh_copy = fleets::gone(&listings, &BTreeMap::from([("whiterabbit".to_owned(), now)]), &BTreeSet::new(), &records, now);
    assert!(fresh_copy.contains("north-293-d1300"), "once a newer copy lacks the session it is forgotten: {fresh_copy:?}");
    let jobs = json!([{"state": "queued", "cmd": ["north", "--by", "nexus", "--name", "north-294-d1200"]}, {"state": "failed", "cmd": ["north", "--name", "north-293-d1300"]}]);
    let queued = fleets::queued_launches(&jobs);
    let waiting = fleets::gone(&listings, &copies, &queued, &records, now);
    assert!(!waiting.contains("north-294-d1200") && record::launching(&records, now, &waiting).contains_key(&before_copy), "north#722 was queued ten times in a minute because its launch, 25 min deep in the laptop's job queue, read as forgotten: {waiting:?}");
}

#[test]
fn a_forgotten_sessions_unread_report_is_no_try_but_an_unread_listed_one_still_counts() {
    let now = 100_000.0;
    let records = [launch("north#255", "north-255-d0900", now - 5.0 * 3600.0), launch("north#255", "north-255-d1100", now - 3.0 * 3600.0)];
    let forgotten = std::collections::BTreeSet::from(["north-255-d0900".to_owned()]);
    let mut notes = Vec::new();
    let unread = |launch: &Record| Err::<Option<serde_json::Value>, String>(format!("error: no session named {}", launch.name));
    let unspent = super::survey::unspent(&records, now, &forgotten, unread, &[], &super::grounds::Ledger::default(), &mut notes);
    assert!(unspent.contains_key("north-255-d0900"), "the forgotten session spends no try: {unspent:?}");
    assert!(!unspent.contains_key("north-255-d1100") && notes.len() == 1, "an unread session that is not forgotten still counts and is noted: {unspent:?} {notes:?}");
    assert!(!record::holds(&records, now, &unspent, &forgotten).values().any(|why| why.contains("tries in 24 h")), "one counted try does not hand the item to the proxy");
}

#[test]
fn a_box_whose_launch_never_started_is_not_launched_again_by_the_next_run() {
    let now = 100_000.0;
    let boxed = |item: &str, name: &str, at: f64, started: bool| Record { box_text: Some("lane sync".into()), launched: started, ..launch(item, name, at) };
    let records = [
        boxed("north#1108", "north-1108-d0650-lane-sync", now - 60.0, false),
        boxed("north#1078", "north-1078-d0650-lane-sync", now - 120.0, true),
        boxed("north#1109", "north-1109-d0650-lane-sync", now - 120.0, true),
        boxed("north#1022", "north-1022-d0650-lane-sync", now - 20.0 * 60.0, false),
    ];
    let gone = BTreeSet::from(["north-1078-d0650-lane-sync".to_owned(), "north-1109-d0650-lane-sync".to_owned()]);
    let unspent = BTreeMap::from([("north-1078-d0650-lane-sync".to_owned(), "exited before any report, a host failure".to_owned())]);
    let held = record::box_launching(&records, now, &unspent, &gone);
    let holds = |number: u64| held.get(&IssueRef::new("north", number)).is_some_and(|boxes| boxes.contains_key("lane-sync"));
    assert!(holds(1108), "a launch that failed to start holds its box for the next run: {held:?}");
    assert!(holds(1078), "a gone session that did no work holds its box: {held:?}");
    assert!(!holds(1109), "a gone session that worked frees its box: {held:?}");
    assert!(!holds(1022), "a failed launch's hold ends after 15 min: {held:?}");
}
