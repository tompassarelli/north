use super::issues::{self, Issue, IssueRef, World};
use super::record;
use super::stops::Stop;
use super::tests::issue;
use std::collections::{BTreeMap, BTreeSet};


#[test]
fn a_last_report_holds_its_box_until_the_issue_changes_after_a_decision_until_its_time_after_a_wait_and_off_the_host_after_a_host_stop() {
    let session = |report: &str| serde_json::json!({
        "entries": [
            {"kind": "assistant", "text": "Working."},
            {"kind": "assistant", "text": report},
            {"kind": "turn", "text": "success"}
        ],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let stop = super::stops::stop(&session("Not done: the two-take limit is reached.\nStop: decision (take three or drop the box)"));
    assert_eq!(stop.decision, Some(1000.0));
    assert_eq!(super::stops::stop(&session("Done: landed abc123.")), Stop::default());
    assert_eq!(super::stops::stop(&session("Not done: no Stop: decision is needed here.")).decision, None, "only a line that begins with it counts");

    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, live: &BTreeSet::new(), planned: &planned, backlog: None, at: 0.0 };
    let box_at = |touched: f64| Issue { stop: stop.clone(), touched, ..issue("makgora", 587, "- [ ] a", &[]) };
    assert_eq!(issues::readiness(&box_at(900.0), &world), Err("last worker stopped for a decision".into()));
    assert_eq!(issues::readiness(&box_at(1000.0), &world), Err("last worker stopped for a decision".into()), "the worker's own comment lands at or before its report");
    assert_eq!(issues::readiness(&box_at(1001.0), &world), Ok(()), "a newer comment or body edit launches it again");
    assert_eq!(issues::readiness(&Issue { stop: Stop::default(), ..box_at(0.0) }, &world), Ok(()));

    let launch = |item: &str, name: &str, at: f64, launched: bool| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched, detail: String::new(), tier: None, category: None, arm: false, hand: false, effort_arm: None, projection: None };
    let records = [launch("makgora#587", "sc-587-d0740", 10.0, true), launch("makgora#587", "sc-587-d0750", 20.0, true), launch("makgora#587", "sc-587-d0800", 30.0, false)];
    assert_eq!(super::stops::last_launches(&records)[&IssueRef::new("makgora", 587)].name, "sc-587-d0750", "a failed launch started no session");

    let world = |at: f64| World { at, ..world };
    let waiting = super::stops::stop(&session("Not done: box 3 counts timeouts over the 24 hours after the fix landed.\nStop: wait until 2026-11-06T23:10Z"));
    assert_eq!(waiting.until, Some(("2026-11-06T23:10Z".to_owned(), 1_794_006_600.0)));
    let held = Issue { stop: waiting, touched: 1_794_000_000.0, ..issue("north", 647, "- [ ] zero timeouts in the 24 hours after the fix", &[]) };
    assert_eq!(issues::readiness(&held, &world(1_793_901_000.0)), Err("its last worker stopped to wait until 2026-11-06T23:10Z".into()), "a later comment does not lift a clock wait");
    assert_eq!(issues::readiness(&held, &world(1_794_006_600.0)), Ok(()), "the box is ready again at that time");

    let elsewhere = super::stops::stop(&session("Not done: the window move has to be checked on the laptop, and gex45 cannot reach it.\nStop: host laptop"));
    assert_eq!(elsewhere.host.as_deref(), Some("laptop"));
    let moved = Issue { stop: elsewhere, ..issue("north", 636, "- [ ] the window moves to the account with headroom", &[]) };
    assert_eq!(issues::readiness(&moved, &world(0.0)), Ok(()), "a host stop is no reason to wait");
    let route = super::tier::Route { tier: super::tier::Tier::Opus, category: "feature".into(), arm: false, effort: None, effort_arm: false, follows: None, why: String::new() };
    let brief = super::brief::brief(&moved, "dispatch", &route, &super::grounds::Grounds::default());
    assert!(brief.lines().any(|line| line == "Needs: laptop"), "{brief}");
    let cfg: toml_edit::DocumentMut = "[hosts.gex45]\nserves = [\"native\"]\n[hosts.whiterabbit]\nserves = [\"laptop\", \"native\"]\n".parse().unwrap();
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", &brief).as_deref(), Some("laptop"), "the host that could not do it is refused");
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "whiterabbit", &brief), None);
    let declared = Issue { body: format!("Needs: native\n{}", moved.body), ..moved };
    assert!(super::brief::brief(&declared, "dispatch", &route, &super::grounds::Grounds::default()).lines().any(|line| line == "Needs: native, laptop"), "the issue's own need stays");
}
