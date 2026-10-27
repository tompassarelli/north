use super::issues::{self, Issue, IssueRef, World};
use super::record;
use super::tests::issue;
use std::collections::{BTreeMap, BTreeSet};


#[test]
fn a_box_whose_last_session_stopped_for_a_decision_is_skipped_until_the_issue_changes() {
    let session = |report: &str| serde_json::json!({
        "entries": [
            {"kind": "assistant", "text": "Working."},
            {"kind": "assistant", "text": report},
            {"kind": "turn", "text": "success"}
        ],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let stopped = super::stops::stopped_at(&session("Not done: the two-take limit is reached.\nStop: decision (take three or drop the box)"));
    assert_eq!(stopped, Some(1000.0));
    assert_eq!(super::stops::stopped_at(&session("Done: landed abc123.")), None);
    assert_eq!(super::stops::stopped_at(&session("Not done: no Stop: decision is needed here.")), None, "only a line that begins with it counts");

    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, planned: &planned, at: 0.0 };
    let box_at = |touched: f64| Issue { stopped, touched, ..issue("smashcraft", 587, "- [ ] a", &[]) };
    assert_eq!(issues::readiness(&box_at(900.0), &world), Err("last worker stopped for a decision".into()));
    assert_eq!(issues::readiness(&box_at(1000.0), &world), Err("last worker stopped for a decision".into()), "the worker's own comment lands at or before its report");
    assert_eq!(issues::readiness(&box_at(1001.0), &world), Ok(()), "a newer comment or body edit launches it again");
    assert_eq!(issues::readiness(&Issue { stopped: None, ..box_at(0.0) }, &world), Ok(()));

    let launch = |item: &str, name: &str, at: f64, launched: bool| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched, detail: String::new(), tier: None, category: None, arm: false, handoff: None };
    let records = [launch("smashcraft#587", "sc-587-d0740", 10.0, true), launch("smashcraft#587", "sc-587-d0750", 20.0, true), launch("smashcraft#587", "sc-587-d0800", 30.0, false)];
    assert_eq!(super::stops::last_launches(&records)[&IssueRef::new("smashcraft", 587)].name, "sc-587-d0750", "a failed launch started no session");
}
