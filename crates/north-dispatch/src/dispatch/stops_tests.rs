use super::issues::{self, Issue, IssueRef, World};
use super::record;
use super::stops::Stop;
use super::tier::{Effort, Tier};
use crate::platform::land_notice::Notice;
use super::tests::issue;
use std::collections::{BTreeMap, BTreeSet};


#[test]
fn a_last_report_holds_its_box_until_the_issue_changes_after_a_decision_until_its_time_after_a_wait_off_the_host_after_a_host_stop_and_one_rung_up_after_a_reasoning_stop_or_a_lane_refused_twice_without_fewer_errors() {
    let session = |report: &str| serde_json::json!({
        "entries": [
            {"kind": "assistant", "text": "Working."},
            {"kind": "assistant", "text": report},
            {"kind": "turn", "text": "success"}
        ],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let launch = |item: &str, name: &str, at: f64, launched: bool| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched, detail: String::new(), tier: None, category: None, hand: false, effort_arm: None, effort: None };
    let ran = launch("makgora#587", "sc-587-d0740", 10.0, true);
    let stop = super::stops::stop(&session("Not done: the two-take limit is reached.\nStop: decision (take three or drop the box)"), &ran);
    assert_eq!(stop.decision, Some(1000.0));
    assert_eq!(super::stops::stop(&session("Done: landed abc123."), &ran), Stop::default());
    assert_eq!(super::stops::stop(&session("Not done: no Stop: decision is needed here."), &ran).decision, None, "only a line that begins with it counts");

    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, live: &BTreeSet::new(), planned: &planned, backlog: None, at: 0.0 };
    let box_at = |touched: f64| Issue { stop: stop.clone(), touched, ..issue("makgora", 587, "- [ ] a", &[]) };
    assert_eq!(issues::readiness(&box_at(900.0), &world), Err("last worker stopped for a decision".into()));
    assert_eq!(issues::readiness(&box_at(1000.0), &world), Err("last worker stopped for a decision".into()), "the worker's own comment lands at or before its report");
    assert_eq!(issues::readiness(&box_at(1001.0), &world), Ok(()), "a newer comment or body edit launches it again");
    assert_eq!(issues::readiness(&Issue { stop: Stop::default(), ..box_at(0.0) }, &world), Ok(()));

    let records = [launch("makgora#587", "sc-587-d0740", 10.0, true), launch("makgora#587", "sc-587-d0750", 20.0, true), launch("makgora#587", "sc-587-d0800", 30.0, false)];
    assert_eq!(super::stops::last_launches(&records)[&IssueRef::new("makgora", 587)].name, "sc-587-d0750", "a failed launch started no session");

    let world = |at: f64| World { at, ..world };
    let waiting = super::stops::stop(&session("Not done: box 3 counts timeouts over the 24 hours after the fix landed.\nStop: wait until 2026-11-06T23:10Z"), &ran);
    assert_eq!(waiting.until, Some(("2026-11-06T23:10Z".to_owned(), 1_794_006_600.0)));
    let held = Issue { stop: waiting, touched: 1_794_000_000.0, ..issue("north", 647, "- [ ] zero timeouts in the 24 hours after the fix", &[]) };
    assert_eq!(issues::readiness(&held, &world(1_793_901_000.0)), Err("its last worker stopped to wait until 2026-11-06T23:10Z".into()), "a later comment does not lift a clock wait");
    assert_eq!(issues::readiness(&held, &world(1_794_006_600.0)), Ok(()), "the box is ready again at that time");

    let elsewhere = super::stops::stop(&session("Not done: the window move has to be checked on the laptop, and gex45 cannot reach it.\nStop: host laptop"), &ran);
    assert_eq!(elsewhere.host.as_deref(), Some("laptop"));
    let moved = Issue { stop: elsewhere, ..issue("north", 636, "- [ ] the window moves to the account with headroom", &[]) };
    assert_eq!(issues::readiness(&moved, &world(0.0)), Ok(()), "a host stop is no reason to wait");
    let route = super::tier::Route { tier: super::tier::Tier::Opus, category: "feature".into(), effort: None, effort_arm: false, follows: None, why: String::new() };
    let brief = super::brief::brief(&moved, "dispatch", &route, &super::grounds::Grounds::default());
    assert!(brief.lines().any(|line| line == "Needs: laptop"), "{brief}");
    let cfg: toml_edit::DocumentMut = "[hosts.gex45]\nserves = [\"native\"]\n[hosts.whiterabbit]\nserves = [\"laptop\", \"native\"]\n".parse().unwrap();
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", &brief).as_deref(), Some("laptop"), "the host that could not do it is refused");
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "whiterabbit", &brief), None);
    let declared = Issue { body: format!("Needs: native\n{}", moved.body), ..moved };
    assert!(super::brief::brief(&declared, "dispatch", &route, &super::grounds::Grounds::default()).lines().any(|line| line == "Needs: native, laptop"), "the issue's own need stays");

    let on = |tier: &str, effort: Option<&str>| record::Record { name: "n705-d0900".into(), tier: Some(tier.into()), effort: effort.map(str::to_owned), ..launch("north#705", "", 10.0, true) };
    let failed = |report: &str, ran: &record::Record| Issue { stop: super::stops::stop(&session(report), ran), touched: 900.0, ..issue("north", 705, "- [ ] a", &[]) };
    let climbed = |issue: &Issue| super::tier::route(issue, &super::tier::History::default(), &BTreeMap::new()).unwrap();
    let reasoning = "Not done: both parser rewrites left the round trip failing.\nStop: reasoning";
    let stuck = failed(reasoning, &on("sonnet-high", None));
    let route = climbed(&stuck);
    assert_eq!((route.tier, route.effort, route.follows.as_deref()), (Tier::Opus, Some(Effort::Medium), Some("n705-d0900 (sonnet-high)")), "{}", route.why);
    let grounds = super::grounds::Grounds::default().carrying(stuck.stop.climb.as_ref().map(|climb| climb.report.as_str()));
    assert!(super::brief::brief(&stuck, "dispatch", &route, &grounds).contains("Prior report: Not done: both parser rewrites"), "the failed report rides along");
    assert_eq!(climbed(&failed(reasoning, &on("worker", Some("high")))).effort, Some(Effort::Xhigh));
    for kind in ["leash", "landing", "capacity", "client"] {
        assert_eq!(failed(&format!("Not done: the box is unfinished.\nStop: {kind}"), &on("sonnet-high", None)).stop.climb, None, "Stop: {kind} never escalates");
    }
    assert_eq!(failed("Done: landed abc123.\nStop: reasoning", &on("sonnet-high", None)).stop.climb, None, "only a Not done report climbs");
    assert!(super::tries::unspent(&session(reasoning)).is_some(), "a climb is no try toward the two that hand the box to the proxy");
    let topped = failed(reasoning, &on("worker", Some("xhigh")));
    assert_eq!(issues::readiness(&topped, &world(0.0)), Err("last worker stopped for a decision".into()), "the top rung parks the box");
    assert_eq!(issues::readiness(&Issue { touched: 1001.0, ..topped }, &world(0.0)), Ok(()), "a decision on the issue relaunches it");

    let lander = |notice: Notice| serde_json::json!({"kind": "user", "text": format!("[from service:north-land@whiterabbit] {}\nlast lines of the log:\nerror[E0308]: mismatched types", notice.text())});
    let refused = |errors: usize| lander(Notice::Refused { branch: "n705-verify".into(), head: "abc".into(), verdict: "failed land check cargo nextest run exited 100; log: /tmp/check.log".into(), errors: Some(errors) });
    let verified = |mut entries: Vec<serde_json::Value>| {
        entries.push(serde_json::json!({"kind": "assistant", "text": "Not done: the lane still fails its checks.\nStop: landing"}));
        Issue { stop: super::stops::stop(&serde_json::json!({"entries": entries, "turns": [{"exit": "completed", "ended_ms": 1_000_000}]}), &on("sonnet-high", None)), touched: 900.0, ..issue("north", 705, "- [ ] a", &[]) }
    };
    let stalled = verified(vec![refused(3), refused(3)]);
    let route = climbed(&stalled);
    assert_eq!((route.tier, route.effort), (Tier::Opus, Some(Effort::Medium)), "{}", route.why);
    assert!(route.why.contains("lane n705-verify, refused twice by its own landing checks without fewer errors (3, then 3)"), "{}", route.why);
    assert!(super::brief::brief(&stalled, "dispatch", &route, &super::grounds::Grounds::default().carrying(stalled.stop.climb.as_ref().map(|climb| climb.report.as_str()))).contains("Prior report: Not done: the lane still fails"), "the failed report rides along");
    assert_eq!(verified(vec![refused(3), refused(2)]).stop.climb, None, "fewer errors keep the rung");
    assert_eq!(verified(vec![refused(3)]).stop.climb, None, "one refusal is no evidence");
    assert_eq!(verified(vec![refused(3), refused(3), lander(Notice::Landed { branch: "n705-verify".into(), sha: "def".into() })]).stop.climb, None, "a landing after clears it");
    let unsigned = serde_json::json!({"kind": "user", "text": Notice::Refused { branch: "n705-verify".into(), head: "abc".into(), verdict: "failed".into(), errors: Some(3) }.text()});
    assert_eq!(verified(vec![refused(3), unsigned]).stop.climb, None, "only the lander's notice counts");
}
