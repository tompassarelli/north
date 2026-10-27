use super::fleets;
use super::issues::{self, IssueRef, World};
use super::plan::Cloud;
use super::record::{self, Record};
use super::tests::{fleet, issue, survey};
use super::{boxless, decide, effects};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn session(tools: &[&str], report: Option<&str>, exit: &str) -> Value {
    let mut entries = vec![json!({"kind": "user", "text": "Fix smashcraft#518."})];
    entries.extend(tools.iter().map(|tool| json!({"kind": "tool", "text": tool})));
    entries.extend(report.map(|text| json!({"kind": "assistant", "text": text})));
    json!({"entries": entries, "turns": [{"exit": exit, "ended_ms": 1_000_000}]})
}

fn launch(item: &str, name: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, arm: false, handoff: None, hand: false }
}

#[test]
fn a_try_whose_session_only_waited_on_admission_does_not_count_toward_the_two_tries_hold() {
    let waited = session(&["Bash wisp lan pool run --pair any -- bun wisp native level-0"], Some("Not done: the level-0 test is still waiting for pair 10."), "completed");
    let deferred = session(&["Bash north capacity run --need native -- wisp lan pool run"], Some("Admission answered DEFER_GPU_CLIENTS for an hour, so nothing ran."), "completed");
    let limited = session(&[], Some("You've hit your session limit · resets 11:50pm (UTC)"), "completed");
    let host_died = session(&[], None, "failed");
    for (spent_nothing, why) in [(&waited, "still waiting for pair 10"), (&deferred, "DEFER_GPU_CLIENTS"), (&limited, "hit your session limit"), (&host_died, "failed before any report")] {
        assert!(super::tries::unspent(spent_nothing).is_some_and(|unspent| unspent.contains(why)), "{spent_nothing}");
    }
    let committed = session(&["Bash git -C ~/code/smashcraft/sc-518 commit -m 'Fix the dash'", "Bash safe-push --to main"], Some("Still in the queue."), "completed");
    let checked = session(&["Bash cargo test --locked --offline"], Some("Waiting on pair 3 for the native half."), "completed");
    let reported = session(&[], Some("Not done: the box asks for two frame counts that contradict each other."), "completed");
    for did_work in [&committed, &checked, &reported] {
        assert_eq!(super::tries::unspent(did_work), None, "a commit, a check run or a report other than a capacity wait spends the try: {did_work}");
    }

    let now = 100_000.0;
    let records = [launch("smashcraft#518", "sc-518-d0900", now - 6.0 * 3600.0), launch("smashcraft#518", "sc-518-d1100", now - 4.0 * 3600.0), launch("smashcraft#552", "sc-552-d0855", now - 5.0 * 3600.0)];
    let contested: Vec<&str> = record::contested(&records, now).iter().map(|record| record.name.as_str()).collect();
    assert_eq!(contested, ["sc-518-d0900", "sc-518-d1100"], "only an item tried twice needs its sessions read");
    let both = record::holds(&records, now, &BTreeMap::new());
    assert_eq!(both[&IssueRef::new("smashcraft", 518)], "north dispatch, 2 tries in 24 h without closing (sc-518-d0900, sc-518-d1100); the proxy decides the next one");
    let unspent = BTreeMap::from([("sc-518-d0900".to_owned(), super::tries::unspent(&waited).unwrap())]);
    assert!(!record::holds(&records, now, &unspent).contains_key(&IssueRef::new("smashcraft", 518)), "one real try is no hold once its launch is old");
    let third = [&records[..], &[launch("smashcraft#518", "sc-518-d1300", now - 3.0 * 3600.0)]].concat();
    let named = &record::holds(&third, now, &unspent)[&IssueRef::new("smashcraft", 518)];
    assert_eq!(named, "north dispatch, 2 tries in 24 h without closing (sc-518-d1100, sc-518-d1300); not counted: sc-518-d0900, which only waited on capacity: Not done: the level-0 test is still waiting for pair 10; the proxy decides the next one");
}

#[test]
fn a_boxless_open_issue_prints_needs_a_box_and_reaches_the_triage_lead() {
    let (hosts, accounts) = fleet();
    let boxes = vec![issue("smashcraft", 525, "The dash skips a frame on wake.", &["bug"]), issue("smashcraft", 5, "Tracker", &["roadmap"]), issue("smashcraft", 11, "- [x] done", &[]), issue("smashcraft", 12, "- [ ] ready", &[])];
    let surveyed = survey(boxes, Cloud::Closed(String::new()), hosts, accounts);
    let outcome = decide(&surveyed, "test");
    let skipped: BTreeMap<String, String> = outcome.skipped.iter().map(|(item, why)| (item.to_string(), why.clone())).collect();
    assert_eq!(skipped["smashcraft#525"], "needs a box: no Done-when box");
    assert_eq!(skipped["smashcraft#5"], "no unticked Done-when box", "a roadmap root needs no box of its own");
    assert_eq!(skipped["smashcraft#11"], "no unticked Done-when box", "a ticked box is finished work, not a missing box");
    let boxless = boxless(&surveyed, &outcome);
    assert_eq!(boxless.iter().map(|issue| issue.reference()).collect::<Vec<_>>(), [IssueRef::new("smashcraft", 525)]);
    assert_eq!(effects::boxless_summary(&boxless), "north dispatch: 1 open issues have no Done-when box, so dispatch cannot staff them; give each a box, a roadmap label, or close it: smashcraft#525 box 525");
}

#[test]
fn a_second_launch_on_a_held_item_is_refused_as_is_a_dispatch_launch_on_an_item_a_manual_task_session_holds() {
    let listing = json!([
        {"name": "sc-518-t1643", "status": "working", "task_ref": "smashcraft#518", "machine": "gex45"},
        {"name": "tom-fix", "status": "idle", "task_ref": "tompassarelli/smashcraft#430", "machine": null},
        {"name": "sc-560-562-illidan", "status": "working", "task_ref": null, "machine": "gex45"},
        {"name": "sc-454-done", "status": "parked", "task_ref": "smashcraft#454", "machine": null}
    ]);
    let sessions: Vec<_> = fleets::running_sessions(&listing).into_iter().map(|session| (session, "whiterabbit".to_owned())).collect();
    let none = BTreeMap::new();
    let held = |number: u64, launching: &BTreeMap<IssueRef, String>| fleets::held(&[IssueRef::new("smashcraft", number)], &sessions, launching).map(|(_, holder)| holder);
    assert_eq!(held(518, &none).as_deref(), Some("fleet session sc-518-t1643 on whiterabbit"), "a second --task launch on #518 is refused");
    assert_eq!(held(562, &none).as_deref(), Some("fleet session sc-560-562-illidan on whiterabbit"), "a session named for two issues holds both");
    assert_eq!(held(454, &none), None, "a parked session holds nothing");
    let now = 100_000.0;
    let records = [launch("smashcraft#552", "sc-552-d0855", now - 600.0), Record { launched: false, ..launch("smashcraft#419", "sc-419-d1006", now - 60.0) }];
    let launching = record::launching(&records, now);
    assert_eq!(held(552, &launching).as_deref(), Some("north dispatch launch sc-552-d0855 on gex45 10 min ago"), "a fresh dispatch launch holds its item before its session shows");
    assert_eq!(held(419, &launching), None, "a failed launch holds nothing");

    let (_, holds) = fleets::tally(&sessions);
    let open = BTreeSet::new();
    let planned = BTreeMap::new();
    let world = World { open: &open, holds: &holds, planned: &planned, at: 0.0 };
    assert_eq!(issues::readiness(&issue("smashcraft", 430, "- [ ] a", &[]), &world), Err("held by fleet session tom-fix on whiterabbit".into()), "dispatch skips an item a manual --task launch holds");
    assert_eq!(issues::readiness(&issue("smashcraft", 562, "- [ ] b", &[]), &world), Err("held by fleet session sc-560-562-illidan on whiterabbit".into()));
}
