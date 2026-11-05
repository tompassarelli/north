use super::fleets;
use super::issues::IssueRef;
use super::record::{self, Record};
use serde_json::json;
use std::collections::BTreeMap;

fn launch(item: &str, name: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, arm: false, handoff: None, hand: false, effort_arm: None, projection: None }
}

#[test]
fn a_laptop_launch_absent_from_a_copy_older_than_it_still_holds_its_item() {
    let now = 100_000.0;
    let on_laptop = |item: &str, name: &str, at: f64| Record { host: "whiterabbit".into(), ..launch(item, name, at) };
    let records = [on_laptop("north#293", "north-293-d1300", now - 40.0 * 60.0), on_laptop("north#294", "north-294-d1200", now - 90.0 * 60.0)];
    let listings = vec![("whiterabbit".to_owned(), json!([{"name": "tom-notes", "status": "idle"}]))];
    let copies = BTreeMap::from([("whiterabbit".to_owned(), now - 60.0 * 60.0)]);
    let gone = fleets::gone(&listings, &copies, &records, now);
    let holds = record::holds(&records, now, &BTreeMap::new(), &gone);
    let launching = record::launching(&records, now, &gone);
    let after_copy = IssueRef::new("north", 293);
    assert!(holds.contains_key(&after_copy) && launching.contains_key(&after_copy), "a copy taken before the launch cannot show its session forgotten: {gone:?}");
    let before_copy = IssueRef::new("north", 294);
    assert!(!holds.contains_key(&before_copy) && !launching.contains_key(&before_copy), "a copy taken well after the launch that lacks its session shows it forgotten: {gone:?}");
    let fresh_copy = fleets::gone(&listings, &BTreeMap::from([("whiterabbit".to_owned(), now)]), &records, now);
    assert!(fresh_copy.contains("north-293-d1300"), "once a newer copy lacks the session it is forgotten: {fresh_copy:?}");
}

#[test]
fn a_launch_queued_behind_an_in_flight_lane_or_held_by_staffing_is_no_launch_failure() {
    let started_nothing = |detail: &str| Record { launched: false, detail: detail.into(), ..launch("north#630", "north-630-d0900", 0.0) };
    assert!(!started_nothing("queued: behind n629-fix, which changed crates/north-dispatch/src/dispatch/effects.rs").failed());
    assert!(!started_nothing("refuse: staffing: north is stalled").failed());
    assert!(started_nothing("claude:b is not signed in on gex45").failed());
}

#[test]
fn a_forgotten_sessions_unread_report_is_no_try_but_an_unread_listed_one_still_counts() {
    let now = 100_000.0;
    let records = [launch("north#255", "north-255-d0900", now - 5.0 * 3600.0), launch("north#255", "north-255-d1100", now - 3.0 * 3600.0)];
    let forgotten = std::collections::BTreeSet::from(["north-255-d0900".to_owned()]);
    let mut notes = Vec::new();
    let unread = |launch: &Record| Err::<serde_json::Value, String>(format!("error: no session named {}", launch.name));
    let unspent = super::survey::unspent(&records, now, &forgotten, unread, &mut notes);
    assert!(unspent.contains_key("north-255-d0900"), "the forgotten session spends no try: {unspent:?}");
    assert!(!unspent.contains_key("north-255-d1100") && notes.len() == 1, "an unread session that is not forgotten still counts and is noted: {unspent:?} {notes:?}");
    assert!(!record::holds(&records, now, &unspent, &forgotten).values().any(|why| why.contains("tries in 24 h")), "one counted try does not hand the item to the proxy");
}
