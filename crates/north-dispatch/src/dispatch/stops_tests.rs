use super::issues::{self, Issue, World};
use super::record;
use super::spin::Evidence;
use crate::platform::land_notice::Notice;
use super::tests::issue;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn a_box_due_by_a_date_or_after_landings_is_held_beside_a_plain_one_north_722() {
    let at = |written: &str| crate::foundation::clock::Timestamp::parse_loose(written).unwrap().unix_seconds_f64();
    let (open, holds, planned, live, no_boxes) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new(), BTreeSet::new(), BTreeMap::new());
    let world = |written: &str| World { open: &open, holds: &holds, box_holds: &no_boxes, box_tries: &no_boxes, live: &live, planned: &planned, backlog: None, at: at(written) };
    let held = |body: &str, landings: usize| issues::readiness(&Issue { landings, ..issue("north", 706, body, &[]) }, &world("2026-11-07T04:50Z"));
    assert_eq!(held("- [ ] the debt is read by 2026-11-09", 0), Err("only open box waits until 2026-11-09".into()));
    assert_eq!(held("- [ ] the debt is read after the next 10 landings", 3), Err("only open box waits until 7 more landings cite it".into()));
    assert_eq!(held("- [ ] the debt is read after the next 10 landings", 10), Ok(()), "ten landings have cited the issue since its last report");
    assert_eq!(held("- [ ] the cache keeps the newest row", 0), Ok(()), "a box with no due phrase is ready");
    assert_eq!(held("- [ ] the debt is read after the next 10 landings\n- [ ] the cache keeps the newest row", 3), Ok(()), "a ready box beside a held one is takeable");
}

#[test]
fn a_box_is_placed_on_its_own_needs_line_and_its_prose_never_holds_it_north_787() {
    let at = 1_794_041_000.0;
    let needs = |body: &str, learned: &[super::host_needs::Learned]| super::host_needs::of(&issue("north", 787, body, &[]), learned, at);
    let prose = "- [ ] the laptop rebuild picks the landing up, read on ccx33 through the fleet tick, in makgora's checkout, as the laptop's niri shows";
    assert!(needs(prose, &[]).is_empty(), "a box with no Needs: line needs nothing, whatever its prose mentions");
    assert!(needs(&format!("{prose} Needs: none"), &[]).is_empty());
    assert_eq!(needs(&format!("{prose} Needs: gex45, native-client"), &[]), ["gex45", "native"], "its own line is its needs");
    assert_eq!(needs("- [ ] watch the timer Needs: laptop", &[]), ["laptop"], "a box that truly needs the laptop stays held");
    let mixed = format!("{prose}\n- [ ] a test pins the order in the crate");
    assert!(needs(&mixed, &[]).is_empty());
    assert!(needs(&format!("{prose}\n- [ ] the timer runs it Needs: laptop"), &[]).is_empty(), "a box that needs nothing places the issue, the worker taking what it can");
    assert_eq!(needs(&format!("{prose} Needs: laptop\n- [ ] the timer runs it Needs: laptop"), &[]), ["laptop"], "only the need every box declares holds");
    let learned = [super::host_needs::Learned { item: "north#787".into(), need: "nexus".into(), session: "n-x".into(), at, boxes: vec![mixed.lines().nth(1).unwrap().to_owned()], reason: String::new() }];
    assert_eq!(needs(&mixed, &learned), ["nexus"], "a learned Stop: host still holds the issue");
    let whole = issue("north", 787, &mixed, &[]);
    assert!(super::host_needs::of_box(&whole, "a test pins the order", &[], at).is_empty());
    assert!(super::host_needs::of_box(&whole, "the laptop rebuild", &[], at).is_empty(), "a hand launch on a box reads that box alone");
}


#[test]
fn a_laptop_launch_reads_its_stop_from_the_reports_the_laptop_mirrors_north_706() {
    let launch = record::Record { at: 1.0, item: "wisp#188".into(), host: "whiterabbit".into(), account: "claude:a".into(), name: "wisp-188-d1646".into(), cloud: false, launched: true, detail: String::new(), tier: Some("sonnet-high".into()), category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None, route: None };
    let refused = serde_json::json!({"kind": "user", "text": format!("[from service:north-land@whiterabbit] {}\nlast lines of the log:", Notice::Refused { branch: "wisp-188".into(), head: "abc".into(), verdict: "the capture check failed".into(), errors: None }.text())});
    let reports = serde_json::json!({"wisp-188-d1646": {"entries": [refused.clone(), refused, {"kind": "assistant", "text": "Not done: the capture still fails."}], "turns": [{"exit": "completed", "ended_ms": 5_000}]}});
    let mut boxes = [issue("wisp", 188, "- [ ] a", &[])];
    let mut notes = Vec::new();

    let listing = serde_json::json!([{"name": "wisp-188-d1646", "status": "parked"}]);
    let laptop = |reports| super::survey::LaptopRegistry { host: "whiterabbit", listing: &listing, reports };
    super::survey::stopped(std::slice::from_ref(&launch), &mut boxes, |launch| super::survey::shown(launch, Some(&laptop(&reports))), |_| None, &super::grounds::Ledger::default(), &mut notes);

    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(boxes[0].stop.climb.as_ref().map(|climb| climb.report.as_str()), Some("Not done: the capture still fails."), "dispatch noted wisp-188-d1646 report unread and could relaunch the box");
    assert!(super::survey::shown(&launch, Some(&laptop(&serde_json::Value::Null))).is_err(), "a report the laptop has not mirrored stays unread");
    let mirrored_none = serde_json::json!({});
    assert!(super::survey::shown(&launch, Some(&laptop(&mirrored_none))).is_err(), "a parked session's report the mirror lacks stays unread");
    let queued = record::Record { name: "north-1059-d0023".into(), ..launch.clone() };
    assert_eq!(super::survey::shown(&queued, Some(&laptop(&mirrored_none))), Ok(None), "north-1059-d0023, still in the laptop's job queue, has no report to read yet");
}

#[test]
fn a_mirrored_session_keeps_what_dispatch_reads_and_reads_the_same_stop_as_the_whole_transcript_north_706() {
    let launch = record::Record { at: 1.0, item: "wisp#188".into(), host: "whiterabbit".into(), account: "claude:a".into(), name: "wisp-188-d1646".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None, route: None };
    let mut entries: Vec<(String, String)> = vec![("user".into(), "Item: wisp#188".into()), ("assistant".into(), "Starting.".into())];
    entries.extend((0..50).map(|n| ("tool".to_owned(), format!("rg thing{n}"))));
    entries.push(("tool".into(), "git commit -m x".into()));
    entries.push(("user".into(), "[from service:north-land] landed wisp-188 as abc123".into()));
    entries.push(("assistant".into(), "Not done: the capture needs the laptop's client.\nStop: wait until 2026-11-08T03:00Z".into()));
    entries.push(("assistant".into(), "The lander's message confirms what I reported.".into()));
    let whole = serde_json::json!({"entries": entries.iter().map(|(kind, text)| serde_json::json!({"kind": kind, "text": text})).collect::<Vec<_>>(), "turns": [{"exit": "completed", "ended_ms": 5_000}]});

    let kept = super::stops::entries_read(&entries);
    let read = serde_json::json!({"entries": kept.iter().map(|(kind, text)| serde_json::json!({"kind": kind, "text": text})).collect::<Vec<_>>(), "turns": [{"exit": "completed", "ended_ms": 5_000}]});

    assert_eq!(kept.len(), 4, "{kept:?}: the first and last assistant messages, the first commit and the lander notice are what dispatch reads, and the 50 other tool calls are not, out of a 38 MB mirror");
    assert_eq!(super::stops::stop(&read, &launch, &Evidence::default()), super::stops::stop(&whole, &launch, &Evidence::default()));
    assert_eq!(super::tries::unspent(&read, &Evidence::default()), super::tries::unspent(&whole, &Evidence::default()));
}

