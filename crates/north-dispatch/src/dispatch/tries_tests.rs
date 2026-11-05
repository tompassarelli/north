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
