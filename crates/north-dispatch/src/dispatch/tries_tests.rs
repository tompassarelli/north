use super::issues::IssueRef;
use super::record::{self, Record};
use std::collections::{BTreeMap, BTreeSet};

fn launch(item: &str, name: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None, route: None }
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
