use super::issues::{self, Issue, IssueRef, World};
use super::record;
use super::spin::Evidence;
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
    let launch = |item: &str, name: &str, at: f64, launched: bool| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    let ran = launch("makgora#587", "sc-587-d0740", 10.0, true);
    let stop = super::stops::stop(&session("Not done: the two-take limit is reached.\nStop: decision (take three or drop the box)"), &ran, &Evidence::default());
    assert_eq!(stop.decision, Some(1000.0));
    assert_eq!(super::stops::stop(&session("Done: landed abc123."), &ran, &Evidence::default()), Stop::default());
    assert_eq!(super::stops::stop(&session("Not done: no Stop: decision is needed here."), &ran, &Evidence::default()).decision, None, "only a line that begins with it counts");

    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, box_holds: &BTreeMap::new(), box_tries: &BTreeMap::new(), live: &BTreeSet::new(), planned: &planned, backlog: None, at: 0.0 };
    let box_at = |touched: f64| Issue { stop: stop.clone(), touched, ..issue("makgora", 587, "- [ ] a", &[]) };
    assert_eq!(issues::readiness(&box_at(900.0), &world), Err("last worker stopped for a decision".into()));
    assert_eq!(issues::readiness(&box_at(1000.0), &world), Err("last worker stopped for a decision".into()), "the worker's own comment lands at or before its report");
    assert_eq!(issues::readiness(&box_at(1001.0), &world), Ok(()), "a newer comment or body edit launches it again");
    assert_eq!(issues::readiness(&Issue { stop: Stop::default(), ..box_at(0.0) }, &world), Ok(()));

    let records = [launch("makgora#587", "sc-587-d0740", 10.0, true), launch("makgora#587", "sc-587-d0750", 20.0, true), launch("makgora#587", "sc-587-d0800", 30.0, false)];
    assert_eq!(super::stops::last_launches(&records)[&IssueRef::new("makgora", 587)].name, "sc-587-d0750", "a failed launch started no session");

    let world = |at: f64| World { at, ..world };
    let waiting = super::stops::stop(&session("Not done: box 3 counts timeouts over the 24 hours after the fix landed.\nStop: wait until 2026-11-06T23:10Z"), &ran, &Evidence::default());
    assert_eq!(waiting.until, Some(("2026-11-06T23:10Z".to_owned(), 1_794_006_600.0)));
    let held = Issue { stop: waiting, touched: 1_794_000_000.0, ..issue("north", 647, "- [ ] zero timeouts in the 24 hours after the fix", &[]) };
    assert_eq!(issues::readiness(&held, &world(1_793_901_000.0)), Err("its last worker stopped to wait until 2026-11-06T23:10Z".into()), "a later comment does not lift a clock wait");
    assert_eq!(issues::readiness(&held, &world(1_794_006_600.0)), Ok(()), "the box is ready again at that time");

    let needs = tempfile::tempdir().unwrap();
    let mut moved = issue("north", 636, "- [ ] the window moves to the account with headroom", &[]);
    let recorded = super::host_needs::record(needs.path(), &moved, None, "sc-636-d0720", "laptop", "the window move has to be checked on the laptop, and gex45 cannot reach it", 1_794_000_000.0).unwrap();
    moved.needs = super::host_needs::of(&moved, &recorded, 1_794_000_000.0);
    assert_eq!(moved.needs, ["laptop"]);
    assert_eq!(super::host_needs::relay(&recorded, "north#636", "sc-636-d0720").map(|relay| (relay.need, relay.report)), Some(("laptop".into(), "the window move has to be checked on the laptop, and gex45 cannot reach it".into())));
    assert_eq!(super::host_needs::relay(&recorded, "north#636", "sc-636-d0730"), None, "only the newest launch's own session relays");
    assert_eq!(super::host_needs::record(needs.path(), &moved, None, "sc-636-d0720", "laptop", "again", 1_794_000_001.0).unwrap().len(), 1, "one record per session and need");
    assert_eq!(issues::readiness(&moved, &world(0.0)), Ok(()), "a host need is no reason to wait");
    let route = super::tier::Route { tier: super::tier::Tier::Opus, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let brief = super::brief::brief(&moved, "dispatch", &route, &super::grounds::Grounds::default(), None);
    assert!(brief.lines().any(|line| line == "Needs: laptop"), "{brief}");
    let cfg: toml_edit::DocumentMut = "[hosts.gex45]\nserves = [\"native\"]\n[hosts.whiterabbit]\nserves = [\"laptop\", \"native\"]\n".parse().unwrap();
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", &brief).as_deref(), Some("laptop"), "the host that could not do it is refused");
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "whiterabbit", &brief), None);
    let declared = Issue { body: format!("Needs: native\n{}", moved.body), ..moved };
    assert!(super::brief::brief(&declared, "dispatch", &route, &super::grounds::Grounds::default(), None).lines().any(|line| line == "Needs: native, laptop"), "the issue's own need stays");

    let hosts: Vec<String> = ["gex45", "ccx33", "nexus", "whiterabbit"].map(str::to_owned).to_vec();
    let cfg: toml_edit::DocumentMut = r#"
[hosts.gex45]
launches = true
serves = ["native:wisp", "warcraft:wisp", "wc3:wisp", "client:wisp", "private:makgora", "native:makgora", "warcraft:makgora", "wc3:makgora", "client:makgora"]
[hosts.ccx33]
launches = true
serves = []
[hosts.nexus]
deploy_ssh = "nexus"
[hosts.whiterabbit]
reaches = ["gex45", "ccx33", "nexus"]
serves = ["private", "native", "warcraft", "wc3", "client", "sign-ins", "local-state", "tui", "fleet", "live", "desktop", "laptop"]
"#.parse().unwrap();
    let d0216 = 1_794_017_760.0;
    let placed = |number: u64, body: &str| {
        let mut issue = issue("north", number, body, &[]);
        issue.needs = super::host_needs::of(&issue, &[], d0216);
        let brief = super::brief::brief(&issue, "dispatch", &route, &super::grounds::Grounds::default(), None);
        hosts.iter().filter(|host| *host != "nexus" && crate::pacing::placement::gate::lacks(&cfg, host, &brief).is_none()).cloned().collect::<Vec<_>>()
    };
    assert_eq!(placed(733, "- [ ] On the laptop (owner: a laptop-placed session; gex45 has no shell there), exercise a proxy take-over: `niri msg -j windows` shows a ghostty window titled `north proxy`"), ["gex45", "ccx33", "whiterabbit"], "a box with no Needs: line is placed on every host, whatever its prose names (north#787)");
    assert_eq!(placed(731, "- [ ] Three consecutive landings go live on all four hosts within 2 minutes each, with a dirty checkout planted during one of them that does not stop the runtime."), ["gex45", "ccx33", "whiterabbit"], "a box with no Needs: line is placed on every host, whatever its prose names (north#787)");
    let goals = "- [ ] makgora's frame goals read the current main: a scheduled job re-runs `bun wisp perf playable-bot-four --samples` (76 s)\n- [ ] makgora's and wisp's goals that stay unmeasured get an oracle or a stated reason\n- [ ] The experiment decisions-never-stop-work is reviewed on 2026-11-23 (registered in north ce12b5de3)";
    assert_eq!(placed(641, goals), ["gex45", "ccx33", "whiterabbit"], "a box with no Needs: line is placed on every host, whatever its prose names (north#787)");
    assert_eq!(placed(630, "- [ ] `north ops check` on Nexus shows launch_failures within 5 over 6 h for a full day after both land. (not before 2026-11-07T00:00Z) The fixes landed as 8f2fde4c7 and 1eb741624 on 2026-11-05."), ["gex45", "ccx33", "whiterabbit"], "a box with no Needs: line is placed on every host, whatever its prose names (north#787)");
    let measured = issue("north", 706, "- [ ] over 24 h no run is attributed host-caused (not before 2026-11-08T00:00Z)\n- [ ] over 6 h no worker run's report names only a measurement that has not come due (not before 2026-11-08T00:00Z)\n- [ ] Clean-up: scaffolding removed.", &[]);
    assert_eq!(issues::readiness(&measured, &world(d0216)), Err("every open box waits, the first until 2026-11-08T00:00Z".into()), "north-706-d0216 was staffed while every box's measurement was not yet due");
    let reported = Issue { stop: super::stops::stop(&session("Not done: every open box is a measurement whose window has not elapsed.\nStop: wait until 2026-11-08T00:00Z"), &ran, &Evidence::default()), ..issue("north", 706, "- [ ] over 24 h no run is attributed host-caused", &[]) };
    assert_eq!(issues::readiness(&reported, &world(d0216)), Err("its last worker stopped to wait until 2026-11-08T00:00Z".into()));
    let gone = |state: &str, body: &str| super::survey::github::gone(&serde_json::json!({"state": state, "body": body}), None);
    assert_eq!(gone("CLOSED", "- [x] a").as_deref(), Some("closed on GitHub, though the mirror listed it open"), "north-736-d0226 launched on an issue closed minutes before, from a stale mirror");
    assert_eq!(gone("OPEN", "- [x] a\n- [x] b").as_deref(), Some("no unticked box on GitHub, though the mirror listed one"));
    assert_eq!(gone("OPEN", "- [x] a\n- [ ] b"), None);
    let copies = tempfile::tempdir().unwrap();
    std::fs::write(
        copies.path().join("threads-whiterabbit.json"),
        r#"[{"item": "north#730", "owner": "n-frontier", "standing": "live"}, {"item": "north#736", "owner": "n-remote-guard", "standing": "live"}, {"item": "north#737", "owner": "n-grant-order", "standing": "live"}, {"item": "north#739", "owner": "n-firn-follow", "standing": "live"}]"#,
    )
    .unwrap();
    let (host, listing) = super::survey::laptop_claims(&cfg, "nexus", copies.path(), d0216, &mut Vec::new()).expect("whiterabbit is the laptop");
    let held = |number: u64, owner: &str| (IssueRef::new("north", number), format!("threads claim by {owner} on whiterabbit"));
    assert_eq!(
        super::survey::claim_rows(&listing, &host, &BTreeSet::new()),
        [held(730, "n-frontier"), held(736, "n-remote-guard"), held(737, "n-grant-order"), held(739, "n-firn-follow")],
        "north-730-d0216 launched while n-frontier on the laptop held north#730; north-736-d0226, north-737-d0241 and north-739-d0245 launched on issues the laptop sessions that filed them now claim on filing"
    );

    let on = |tier: &str, effort: Option<&str>| record::Record { name: "n705-d0900".into(), tier: Some(tier.into()), effort: effort.map(str::to_owned), ..launch("north#705", "", 10.0, true) };
    let failed = |report: &str, ran: &record::Record| Issue { stop: super::stops::stop(&session(report), ran, &Evidence::default()), touched: 900.0, ..issue("north", 705, "- [ ] a", &[]) };
    let climbed = |issue: &Issue| super::tier::route(issue, &super::tier::History::default(), &BTreeMap::new()).unwrap();
    let reasoning = "Not done: both parser rewrites left the round trip failing.\nStop: reasoning";
    let stuck = failed(reasoning, &on("sonnet-high", None));
    let route = climbed(&stuck);
    assert_eq!((route.tier, route.effort, route.follows.as_deref()), (Tier::Opus, Some(Effort::Medium), Some("n705-d0900 (sonnet-high)")), "{}", route.why);
    let grounds = super::grounds::Grounds::default().carrying(stuck.stop.climb.as_ref().map(|climb| climb.report.as_str()));
    assert!(super::brief::brief(&stuck, "dispatch", &route, &grounds, None).contains("Prior report: Not done: both parser rewrites"), "the failed report rides along");
    assert_eq!(climbed(&failed(reasoning, &on("worker", Some("high")))).effort, Some(Effort::Xhigh));
    for kind in ["leash", "landing", "capacity", "client"] {
        assert_eq!(failed(&format!("Not done: the box is unfinished.\nStop: {kind}"), &on("sonnet-high", None)).stop.climb, None, "Stop: {kind} never escalates");
    }
    assert_eq!(failed("Done: landed abc123.\nStop: reasoning", &on("sonnet-high", None)).stop.climb, None, "only a Not done report climbs");
    assert!(super::tries::unspent(&session(reasoning), &Evidence::default()).is_some(), "a climb is no try toward the two that hand the box to the proxy");
    let safety = serde_json::json!({"entries": [{"kind": "refusal", "text": "safety refusal (cyber: claude-opus-5 -> claude-opus-4-8)"}, {"kind": "assistant", "text": reasoning}], "turns": [{"exit": "completed", "ended_ms": 1_000_000}]});
    let refused = Issue { stop: super::stops::stop(&safety, &on("sonnet-high", None), &Evidence::default()), touched: 900.0, ..issue("north", 705, "- [ ] a", &[]) };
    assert_eq!(refused.stop.climb, None, "a safety refusal is a boundary row, not a reasoning failure that climbs");
    assert_eq!(climbed(&refused).signal, None, "a safety refusal relaunches on the same step or parks, and never escalates");
    let topped = failed(reasoning, &on("worker", Some("max")));
    assert_eq!(issues::readiness(&topped, &world(0.0)), Err("last worker stopped for a decision".into()), "the top rung parks the box");
    assert_eq!(issues::readiness(&Issue { touched: 1001.0, ..topped }, &world(0.0)), Ok(()), "a decision on the issue relaunches it");

    let lander = |notice: Notice| serde_json::json!({"kind": "user", "text": format!("[from service:north-land@whiterabbit] {}\nlast lines of the log:\nerror[E0308]: mismatched types", notice.text())});
    let refused = |errors: usize| lander(Notice::Refused { branch: "n705-verify".into(), head: "abc".into(), verdict: "failed land check cargo nextest run exited 100; log: /tmp/check.log".into(), errors: Some(errors) });
    let verified = |mut entries: Vec<serde_json::Value>| {
        entries.push(serde_json::json!({"kind": "assistant", "text": "Not done: the lane still fails its checks.\nStop: landing"}));
        Issue { stop: super::stops::stop(&serde_json::json!({"entries": entries, "turns": [{"exit": "completed", "ended_ms": 1_000_000}]}), &on("sonnet-high", None), &Evidence::default()), touched: 900.0, ..issue("north", 705, "- [ ] a", &[]) }
    };
    let stalled = verified(vec![refused(3), refused(3)]);
    let route = climbed(&stalled);
    assert_eq!((route.tier, route.effort), (Tier::Opus, Some(Effort::Medium)), "{}", route.why);
    assert!(route.why.contains("lane n705-verify, refused twice by its own landing checks without fewer errors (3, then 3)"), "{}", route.why);
    assert!(super::brief::brief(&stalled, "dispatch", &route, &super::grounds::Grounds::default().carrying(stalled.stop.climb.as_ref().map(|climb| climb.report.as_str())), None).contains("Prior report: Not done: the lane still fails"), "the failed report rides along");
    assert_eq!(verified(vec![refused(3), refused(2)]).stop.climb, None, "fewer errors keep the rung");
    assert_eq!(verified(vec![refused(3)]).stop.climb, None, "one refusal is no evidence");
    assert_eq!(verified(vec![refused(3), refused(3), lander(Notice::Landed { branch: "n705-verify".into(), sha: "def".into() })]).stop.climb, None, "a landing after clears it");
    let unsigned = serde_json::json!({"kind": "user", "text": Notice::Refused { branch: "n705-verify".into(), head: "abc".into(), verdict: "failed".into(), errors: Some(3) }.text()});
    assert_eq!(verified(vec![refused(3), unsigned]).stop.climb, None, "only the lander's notice counts");
}

#[test]
fn dispatch_retries_opus_xhigh_once_then_runs_opus_max_once_and_parks_the_box_after_a_failed_max() {
    use super::tier::{History, Rung};
    let report = "Not done: the parser rewrite still fails the round trip.\nStop: reasoning";
    let session = serde_json::json!({
        "entries": [
            {"kind": "assistant", "text": "Working."},
            {"kind": "assistant", "text": report}
        ],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let launched = |name: &str, effort: &str| record::Record {
        at: 10.0,
        item: "north#940".into(),
        host: "gex45".into(),
        account: "claude:c".into(),
        name: name.into(),
        cloud: false,
        launched: true,
        detail: String::new(),
        tier: Some("worker".into()),
        category: Some("feature".into()),
        box_text: None,
        hand: false,
        signal: None,
        lift: None,
        effort_arm: None,
        effort: Some(effort.into()),
        cell: None,
        cell_role: None,
    };
    let failed = |ran: &record::Record| Issue { stop: super::stops::stop(&session, ran, &Evidence::default()), touched: 900.0, ..issue("north", 940, "- [ ] a", &[]) };
    let first = launched("n940-xhigh-1", "xhigh");
    let second = launched("n940-xhigh-2", "xhigh");
    let max = launched("n940-max", "max");

    let retry = super::tier::route(&failed(&first), &History::new(std::slice::from_ref(&first)), &BTreeMap::new()).unwrap();
    assert_eq!((retry.tier, retry.effort), (Tier::Opus, Some(Effort::Xhigh)), "{}", retry.why);
    assert!(retry.why.contains("retries n940-xhigh-1") && retry.follows.as_deref() == Some("n940-xhigh-1 (worker at xhigh effort)"), "{}", retry.why);

    let climb = super::tier::route(&failed(&second), &History::new(&[first, second]), &BTreeMap::new()).unwrap();
    assert_eq!((climb.tier, climb.effort), (Tier::Opus, Some(Effort::Max)), "{}", climb.why);
    assert_eq!(climb.follows.as_deref(), Some("n940-xhigh-2 (worker at xhigh effort)"));
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    assert_eq!(climb.invocation(&cfg).unwrap(), ["--model", "claude-opus-5-5", "--effort", "max"].map(str::to_owned));

    let parked = failed(&max);
    assert_eq!(parked.stop.climb.as_ref().map(|climb| climb.rung), Some(Rung::of(Tier::Opus, Some(Effort::Max))), "the max launch is recorded as the climb's top rung");
    let world = World { open: &BTreeSet::new(), holds: &BTreeMap::new(), box_holds: &BTreeMap::new(), box_tries: &BTreeMap::new(), live: &BTreeSet::new(), planned: &BTreeMap::new(), backlog: None, at: 1000.0 };
    assert_eq!(issues::readiness(&parked, &world), Err("last worker stopped for a decision".into()), "a failed max try parks the box for a decision");
    assert_eq!(issues::readiness(&Issue { touched: 1001.0, ..parked }, &world), Ok(()), "a newer decision relaunches it");
}

#[test]
fn north_725_d0411_is_held_off_gex45_by_the_host_needs_both_earlier_launches_recorded() {
    let body = "- [ ] the issue holds the per-host byte table with under 5% unaccounted.\n- [ ] gex45 sits under its low watermark after one cleanup pass (df output), and a replay of the 99d4ba44d1 condition trims caches before any land slot.\n- [x] the change is on origin/main";
    let directory = tempfile::tempdir().unwrap();
    let (d0306_at, d0352_at, d0411_at) = (1_794_027_996.0, 1_794_030_733.0, 1_794_033_106.0);
    let first = issue("north", 725, body, &[]);
    super::host_needs::record(directory.path(), &first, None, "north-725-d0306", "ccx33", "the ledger fix landed, but ccx33 and the laptop were not measured", d0306_at + 2100.0).unwrap();
    let learned = super::host_needs::record(directory.path(), &first, None, "north-725-d0352", "laptop", "the laptop row of the per-host byte table needs `north capacity disk` run on the laptop itself", d0352_at + 600.0).unwrap();
    let mut issue = first;
    issue.needs = super::host_needs::of(&issue, &learned, d0411_at);
    assert_eq!(super::host_needs::relay(&learned, "north#725", "north-725-d0352").map(|relay| relay.need).as_deref(), Some("laptop"), "the newest launch's own need is the one the next launch follows");
    let route = super::tier::Route { tier: Tier::Sonnet, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let brief = super::brief::brief(&issue, "dispatch", &route, &super::grounds::Grounds::default(), None);
    assert!(brief.lines().any(|line| line == "Needs: ccx33, laptop"), "north-725-d0306's recorded need ccx33 still holds, though a later launch ran since:\n{brief}");
    let cfg: toml_edit::DocumentMut = r#"
[hosts.gex45]
launches = true
serves = ["native:wisp", "warcraft:wisp", "wc3:wisp", "client:wisp", "private:makgora", "native:makgora", "warcraft:makgora", "wc3:makgora", "client:makgora"]
[hosts.ccx33]
launches = true
serves = []
[hosts.cpx62]
launches = true
serves = []
[hosts.whiterabbit]
reaches = ["gex45", "ccx33", "cpx62", "nexus"]
serves = ["private", "native", "warcraft", "wc3", "client", "sign-ins", "local-state", "tui", "fleet", "live", "desktop", "laptop"]
"#.parse().unwrap();
    let lacks = |host: &str| crate::pacing::placement::gate::lacks(&cfg, host, &brief);
    assert_eq!(lacks("gex45").as_deref(), Some("laptop"), "north-725-d0411 went to gex45 and recorded a laptop need");
    assert_eq!((lacks("ccx33").as_deref(), lacks("cpx62").as_deref(), lacks("whiterabbit")), (Some("laptop"), Some("laptop"), None));
    let ticked = Issue { body: body.replace("- [ ] the issue holds", "- [x] the issue holds").replace("- [ ] gex45 sits", "- [x] gex45 sits"), ..issue };
    assert!(super::host_needs::of(&ticked, &super::host_needs::read(directory.path()), d0411_at).is_empty(), "a need goes once every box it was learned for is ticked");
}

#[test]
fn a_hand_launch_on_north_759_reads_the_nexus_need_of_n_claim_reland_and_gex45_is_refused_while_an_item_with_no_need_places_as_before() {
    let at = 1_794_041_000.0;
    let directory = tempfile::tempdir().unwrap();
    let claimed = issue("north", 759, "- [ ] the claim lands on the ledger host and a replay shows it held", &[]);
    let learned = super::host_needs::record(directory.path(), &claimed, None, "n-claim-reland", "nexus", "the claim reland needs the ledger on nexus", at).unwrap();
    let needs = super::host_needs::of(&claimed, &learned, at);
    assert_eq!(needs, ["nexus"], "the hand launch's own recorded need places the next try like a dispatched one's");

    let cfg: toml_edit::DocumentMut = r#"
[hosts.gex45]
launches = true
serves = []
[hosts.nexus]
launches = true
serves = []
[hosts.whiterabbit]
reaches = ["gex45", "nexus"]
serves = ["laptop"]
"#.parse().unwrap();
    let brief = "Item: north#759 Reland the claim\nRepository: north\nNeeds: native";
    let placed = crate::pacing::placement::needs::with_needs(brief, &needs);
    assert!(placed.starts_with("Needs: native, nexus\n"), "{placed}");
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", &placed).as_deref(), Some("native"), "a local need is named before an unreached host");
    let unneeding = "Item: north#759 Reland the claim\nRepository: north";
    let placed = crate::pacing::placement::needs::with_needs(unneeding, &needs);
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", &placed).as_deref(), Some("reach to nexus"));
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "whiterabbit", &placed), None);

    let plain = issue("north", 760, "- [ ] a", &[]);
    let none = super::host_needs::of(&plain, &[], at);
    assert!(none.is_empty());
    assert_eq!(crate::pacing::placement::needs::with_needs(unneeding, &none), unneeding);
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", unneeding), None);

}

#[test]
fn north_751_at_12_02_pm_is_held_until_its_1_h_cell_can_be_read_an_hour_after_the_newest_landing_it_cites() {
    let at = |written: &str| crate::foundation::clock::Timestamp::parse_loose(written).unwrap().unix_seconds_f64();
    let body = "- [x] Sessions the proxy names and places itself pass the same box-needs placement, fixed by 745f9b42a\n- [x] A run ending on the API's \"Credit balance is too low\" counts as capacity, fixed by 3a3a29687\n- [ ] the 10 min and 1 h cells reach 1% or less, quoted with their n; the capacity losses left are sessions stopped while their own build waited in admission, worked on in north#772";
    let held = Issue { landed: at("2026-11-07T04:45Z"), touched: at("2026-11-07T05:03Z"), ..issue("north", 751, body, &[]) };
    let (open, holds, planned, live, no_boxes) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new(), BTreeSet::new(), BTreeMap::new());
    let world = |written: &str| World { open: &open, holds: &holds, box_holds: &no_boxes, box_tries: &no_boxes, live: &live, planned: &planned, backlog: None, at: at(written) };
    assert_eq!(held.cited_shas(), ["745f9b42a", "3a3a29687"]);
    assert_eq!(
        issues::readiness(&held, &world("2026-11-07T05:02Z")),
        Err("no open box can be read before 2026-11-07T05:45Z: each names a window over readings, counted from the newest landing it cites or its last report".into()),
        "05:45Z is 12:45 PM on the laptop"
    );
    assert_eq!(issues::readiness(&held, &world("2026-11-07T05:45Z")), Ok(()), "the box is takeable once the window has turned over");
    let unwindowed = Issue { landed: held.landed, ..issue("north", 752, "- [ ] the cache keeps the newest row", &[]) };
    assert_eq!(issues::readiness(&unwindowed, &world("2026-11-07T05:02Z")), Ok(()), "a box that names no window is not held");
}

#[test]
fn a_hand_launch_for_north_757s_session_lifecycle_box_is_not_placed_on_the_ccx33_stop_another_box_learned_while_a_launch_with_no_box_line_reads_every_open_box() {
    let sessions_box = "- [ ] **1. Fleet session lifecycle and retirement** (score 348) Files: crates/north-fleet/src/fleet/model.rs. Done when: `cargo nextest run -p north-fleet --lib -E 'test(/session_lifecycle/)'` passes";
    let leases_box = "- [ ] **9. Capacity leases** (score 82) Files: crates/north-capacity-host/src/capacity/host/state.rs. Done when: `cargo nextest run -p north-capacity-host --lib -E 'test(/lease_lifecycle/)'` passes";
    let body = format!("{sessions_box}\n{leases_box}");
    let at = 1_794_045_000.0;
    let learned = [super::host_needs::Learned { item: "north#757".into(), need: "ccx33".into(), session: "n-leases".into(), at: at - 3600.0, boxes: vec![leases_box.to_owned()], reason: String::new() }];
    let claimed = issue("north", 757, &body, &[]);
    let cfg: toml_edit::DocumentMut = r#"
[hosts.ccx33]
launches = true
serves = []
[hosts.cpx62]
launches = true
serves = []
[hosts.whiterabbit]
reaches = ["ccx33", "cpx62"]
serves = ["laptop"]
"#.parse().unwrap();
    let brief = "Item: north#757 sessions reland\nBox: **1. Fleet session lifecycle and retirement**\nRepository: north";
    let lacks = |needs: &[String]| crate::pacing::placement::gate::lacks(&cfg, "cpx62", &crate::pacing::placement::needs::with_needs(brief, needs));

    let whole = super::host_needs::of(&claimed, &learned, at);
    assert_eq!(whole, ["ccx33"]);
    assert_eq!(lacks(&whole).as_deref(), Some("reach to ccx33"), "n-loss-lead's launch of cpx62-sessions-reland read the other box's stop");

    let only = super::host_needs::of_box(&claimed, super::facets::declared_in(brief, "Box").unwrap(), &learned, at);
    assert!(only.is_empty(), "{only:?}");
    assert_eq!(lacks(&only), None);

    let leases = super::host_needs::of_box(&claimed, "**9. Capacity leases**", &learned, at);
    assert_eq!(leases, ["ccx33"], "the box that learned the stop still reads it");
    assert_eq!(super::host_needs::of_box(&claimed, "a box no line holds", &learned, at), whole, "an unmatched Box: line reads every open box");
}

#[test]
fn north_722_waited_twice_on_one_stop_and_a_box_due_by_a_date_or_after_landings_is_held_beside_a_plain_one() {
    let at = |written: &str| crate::foundation::clock::Timestamp::parse_loose(written).unwrap().unix_seconds_f64();
    let launch = |name: &str, started: &str| record::Record { at: at(started), item: "north#722".into(), host: "cpx62".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    let waited = serde_json::json!({
        "entries": [{"kind": "assistant", "text": "Not done: the debt box reads after the landings.\nStop: wait until 2026-11-07T10:00Z"}],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let body = "- [ ] the after-landing debt is under 5 boxes";
    let directory = tempfile::tempdir().unwrap();
    let (open, holds, planned, live, no_boxes) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new(), BTreeSet::new(), BTreeMap::new());
    let world = |written: &str| World { open: &open, holds: &holds, box_holds: &no_boxes, box_tries: &no_boxes, live: &live, planned: &planned, backlog: None, at: at(written) };
    let pass = |records: &[record::Record], readable: bool, now: &str| {
        let mut boxes = [issue("north", 722, body, &[])];
        let show = |_: &record::Record| if readable { Ok(waited.clone()) } else { Err::<serde_json::Value, String>("no session".into()) };
        super::survey::stopped(records, &mut boxes, show, |_| None, &super::grounds::Ledger::default(), &mut Vec::new());
        let launches = super::stops::last_launches(records);
        super::wait_stops::learn(directory.path(), &boxes, |issue| launches.get(&issue.reference()).map(|launch| launch.name.clone()), at(now)).unwrap();
        let [mut issue] = boxes;
        issue.learned_wait = super::wait_stops::holding(&super::wait_stops::read(directory.path()), &issue, at(now));
        issues::readiness(&issue, &world(now))
    };
    let (first, second) = (launch("north-722-d0300", "2026-11-07T03:00Z"), launch("north-722-d0421", "2026-11-07T04:21Z"));
    let held_until_ten = Err("its last worker stopped to wait until 2026-11-07T10:00Z".to_owned());
    assert_eq!(pass(std::slice::from_ref(&first), true, "2026-11-07T03:30Z"), held_until_ten);
    assert_eq!(pass(&[first.clone(), second.clone()], false, "2026-11-07T04:21Z"), held_until_ten, "the second launch's report is unread, and the wait the first learned still holds the box");
    assert_eq!(pass(&[first, second], true, "2026-11-07T04:50Z"), held_until_ten);
    assert_eq!(super::wait_stops::read(directory.path()).len(), 2, "one record per session that stopped on the wait");
    assert_eq!(pass(&[], false, "2026-11-07T10:00Z"), Ok(()), "the box is ready once the wait has passed");

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
fn the_relaunch_after_north_733s_recorded_laptop_need_follows_the_stopped_run_and_starts_from_its_reason() {
    let mut stopped = issue("north", 733, "- [ ] a take-over is watched live", &[]);
    let recorded = super::host_needs::record(tempfile::tempdir().unwrap().path(), &stopped, None, "north-733-d0215", "laptop", "the code for north#733 is already on origin/main, but nobody has watched a take-over live", 10.0).unwrap();
    stopped.needs = super::host_needs::of(&stopped, &recorded, 10.0);
    stopped.stop.relay = super::host_needs::relay(&recorded, "north#733", "north-733-d0215");

    let route = super::tier::route(&stopped, &super::tier::History::default(), &BTreeMap::new()).unwrap();
    let grounds = super::grounds::Grounds::default().carrying(stopped.stop.relay.as_ref().map(|relay| relay.report.as_str()));
    let brief = super::brief::brief(&stopped, "dispatch", &route, &grounds, None);

    assert!(brief.lines().any(|line| line == "Follows: north-733-d0215 (Stop: host laptop)"), "{brief}");
    assert!(brief.lines().any(|line| line == "Needs: laptop"), "{brief}");
    assert!(brief.contains("Prior report: the code for north#733 is already on origin/main"), "{brief}");
    let fresh = Issue { stop: Stop::default(), ..stopped };
    assert_eq!(super::tier::route(&fresh, &super::tier::History::default(), &BTreeMap::new()).unwrap().follows, None, "a box with no host stop follows nothing");
}

#[test]
fn a_laptop_launch_reads_its_stop_from_the_reports_the_laptop_mirrors_north_706() {
    let launch = record::Record { at: 1.0, item: "wisp#188".into(), host: "whiterabbit".into(), account: "claude:a".into(), name: "wisp-188-d1646".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    let reports = serde_json::json!({"wisp-188-d1646": {"entries": [{"kind": "assistant", "text": "Not done: the capture needs the laptop's client.\nStop: wait until 2026-11-08T03:00Z"}], "turns": [{"exit": "completed", "ended_ms": 5_000}]}});
    let mut boxes = [issue("wisp", 188, "- [ ] a", &[])];
    let mut notes = Vec::new();

    super::survey::stopped(std::slice::from_ref(&launch), &mut boxes, |launch| super::survey::shown(launch, Some("whiterabbit"), &reports), |_| None, &super::grounds::Ledger::default(), &mut notes);

    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(boxes[0].stop.until.as_ref().map(|(written, _)| written.as_str()), Some("2026-11-08T03:00Z"), "dispatch noted wisp-188-d1646 report unread and could relaunch the box");
    assert!(super::survey::shown(&launch, Some("whiterabbit"), &serde_json::Value::Null).is_err(), "a report the laptop has not mirrored stays unread");
}

#[test]
fn a_mirrored_session_keeps_what_dispatch_reads_and_reads_the_same_stop_as_the_whole_transcript_north_706() {
    let launch = record::Record { at: 1.0, item: "wisp#188".into(), host: "whiterabbit".into(), account: "claude:a".into(), name: "wisp-188-d1646".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    let mut entries: Vec<(String, String)> = vec![("user".into(), "Item: wisp#188".into()), ("assistant".into(), "Starting.".into())];
    entries.extend((0..50).map(|n| ("tool".to_owned(), format!("rg thing{n}"))));
    entries.push(("tool".into(), "git commit -m x".into()));
    entries.push(("user".into(), "[from service:north-land] landed wisp-188 as abc123".into()));
    entries.push(("assistant".into(), "Not done: the capture needs the laptop's client.\nStop: wait until 2026-11-08T03:00Z".into()));
    entries.push(("assistant".into(), "The lander's message confirms what I reported.".into()));
    let whole = serde_json::json!({"entries": entries.iter().map(|(kind, text)| serde_json::json!({"kind": kind, "text": text})).collect::<Vec<_>>(), "turns": [{"exit": "completed", "ended_ms": 5_000}]});

    let kept = super::stops::entries_read(&entries);
    let read = serde_json::json!({"entries": kept.iter().map(|(kind, text)| serde_json::json!({"kind": kind, "text": text})).collect::<Vec<_>>(), "turns": [{"exit": "completed", "ended_ms": 5_000}]});

    assert_eq!(kept.len(), 5, "{kept:?}: the three assistant messages, the first commit and the lander notice are what dispatch reads, and the 50 other tool calls are not, out of a 38 MB mirror");
    assert_eq!(super::stops::stop(&read, &launch, &Evidence::default()), super::stops::stop(&whole, &launch, &Evidence::default()));
    assert_eq!(super::tries::unspent(&read, &Evidence::default()), super::tries::unspent(&whole, &Evidence::default()));
}

/// north#903: on 2026-11-07 and 08 four first tries were lost to boxes staffed before they could be done: their wait counted from a box above that was still unticked, or from a count with no date.
#[test]
fn a_box_whose_wait_counts_from_an_unticked_box_above_or_from_an_undated_count_is_not_staffed_while_a_plain_box_beside_it_is() {
    let at = |written: &str| crate::foundation::clock::Timestamp::parse_loose(written).unwrap().unix_seconds_f64();
    let (open, holds, planned, live, no_boxes) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new(), BTreeSet::new(), BTreeMap::new());
    let world = |written: &str| World { open: &open, holds: &holds, box_holds: &no_boxes, box_tries: &no_boxes, live: &live, planned: &planned, backlog: None, at: at(written) };
    let ticked = "- [x] A verb lands and its test passes, fixed by 2d663c0f2e";
    let plain = "- [ ] the cache keeps the newest row";
    let enforce_865 = "- [ ] After 24 h or 20 decisions in shadow, a sample of 20 of its reports reads at most 2 false, recorded in harness/gates.toml's review table, and it enforces.";
    let revert_865 = "- [ ] Keep-or-revert, read 7 days after it enforces: the Rust commits of those 7 days add comment lines net of those they remove at no more than 2 lines per 100 commits (13 per 100 from 2026-11-06 to 08). Otherwise revert the rule.";
    let enforce_872 = "- [ ] Enforce after review: a reviewer reads 24 h or 20 of the shadow rows, fixes any false refusal and records `[gate.unrunnable-box.review]`. Done when: `rg -q mode harness/gates.toml`.";
    let shadow_872 = "- [ ] Shadow: the issue-filing hook logs each new unticked box that names no runnable command. Done when: `rg -q unrunnable-box harness/gates.toml`.";
    let report_877 = "- [ ] After 30 landed per arm, `north test frontier ab-report` writes into docs/rationale/frontier.md, per arm, the median assignment-to-landed time.";
    let assign_877 = "- [ ] A verb picks open boxes with a runnable Done when and assigns each by a recorded seeded coin flip to an arm.";
    let journal_637 = "- [ ] `journalctl --user -u fleet-tick.service` on the laptop shows no start timeout for a day after it lands. Check missed on 2026-11-07, printed \"38\", expected \"0\" <!-- check n637-day-without-timeouts@2026-11-07 missed -->";
    let landed = at("2026-11-05T23:02Z");
    let takeable = |number: u64, body: &str, now: &str| {
        let candidate = Issue { landed, touched: landed, ..issue("north", number, body, &[]) };
        candidate.takeable(at(now)).map(|line| line.trim().chars().take(24).collect::<String>()).collect::<Vec<_>>()
    };
    let head = |line: &str| line.chars().take(24).collect::<String>();

    let launch_865 = format!("{ticked}\n{enforce_865}\n{revert_865}\n{plain}");
    assert_eq!(takeable(865, &launch_865, "2026-11-07T23:26Z"), [head(plain)], "north-865-d2326 was staffed on the keep-or-revert box while the enforce box above it was unticked");
    let launch_872 = format!("{ticked}\n{shadow_872}\n{enforce_872}\n{plain}");
    assert_eq!(takeable(872, &launch_872, "2026-11-08T00:09Z"), [head(shadow_872), head(plain)], "north-872-d0006 was staffed on the enforce box while the shadow box above it had not landed");
    let launch_877 = format!("{ticked}\n{assign_877}\n{report_877}\n{plain}");
    assert_eq!(takeable(877, &launch_877, "2026-11-08T01:13Z"), [head(assign_877), head(plain)], "north-877-d0111 was staffed on the report box while the assign-and-record box above it had not landed");
    let launch_637 = format!("{ticked}\n{journal_637}\n{plain}");
    assert_eq!(takeable(637, &launch_637, "2026-11-07T18:29Z"), [head(plain)], "north-637-d1828 was staffed on a box reopened by a missed check the same day");

    assert_eq!(issues::readiness(&Issue { landed, ..issue("north", 872, &format!("{ticked}\n{shadow_872}\n{enforce_872}"), &[]) }, &world("2026-11-08T00:09Z")), Ok(()), "the shadow box is staffable");
    let waiting = |body: &str| issues::readiness(&Issue { landed, ..issue("north", 903, body, &[]) }, &world("2026-11-08T00:09Z"));
    assert_eq!(
        waiting(&format!("{ticked}\n{enforce_865}\n{revert_865}")),
        Err("every open box waits, the first until it is stamped `(not before YYYY-MM-DDTHH:MMZ)`: it counts something with no date".into())
    );
    assert_eq!(waiting(&format!("{ticked}\n{journal_637}")), Err("only open box waits until it is stamped `(not before YYYY-MM-DDTHH:MMZ)`: it counts something with no date".into()));
    let stamped = "- [ ] Keep-or-revert, read 7 days after it enforces (not before 2026-11-16T00:00Z): the commits add no more comments";
    let behind = waiting(&format!("{ticked}\n{stamped}\n{revert_865}")).unwrap_err();
    assert!(behind.contains("the unticked box above it is done: `Keep-or-revert, read 7 days after it enforces (not before"), "{behind}");

    assert_eq!(takeable(865, &format!("{enforce_865}\n{stamped}"), "2026-11-16T00:00Z"), [head(stamped)], "a box stamped with its own date is read from that date, as before");
    assert_eq!(takeable(865, "- [ ] the cache keeps the newest row, read over 6 h\n- [ ] the debt is read after the next 10 landings", "2026-11-16T00:00Z"), [head(plain)], "a window over readings is not a wait on the box above");
}

fn spinning(report: &str, notices: &[String], evidence: &Evidence) -> Issue {
    let mut entries: Vec<serde_json::Value> = notices.iter().map(|text| serde_json::json!({"kind": "user", "text": format!("[from service:north-land@whiterabbit] {text}\nlast lines of the log:")})).collect();
    entries.push(serde_json::json!({"kind": "assistant", "text": report}));
    let session = serde_json::json!({"entries": entries, "turns": [{"exit": "completed", "ended_ms": 1_000_000}]});
    let launch = record::Record { at: 10.0, item: "north#940".into(), host: "gex45".into(), account: "claude:c".into(), name: "n940-d0900".into(), cloud: false, launched: true, detail: String::new(), tier: Some("sonnet-high".into()), category: Some("feature".into()), box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    Issue { stop: super::stops::stop(&session, &launch, evidence), touched: 900.0, ..issue("north", 940, "- [ ] a", &[]) }
}

#[test]
fn the_eighteen_minute_candidate_check_climbs_the_box_one_rung_at_its_eta_overrun() {
    use super::spin::Signal;
    let report = "Not done: the lander's candidate check held the box past its window.\nStop: landing";
    let over = Evidence { reopened: false, time: Some(crate::ledger::RunTime { minutes: 36, eta_min: 18 }) };
    let route = super::tier::route(&spinning(report, &[], &over), &super::tier::History::default(), &BTreeMap::new()).unwrap();
    assert_eq!((route.tier, route.effort, route.signal), (Tier::Opus, Some(Effort::Medium), Some(Signal::EtaOverrun)), "{}", route.why);
    assert!(route.why.contains("escalates n940-d0900's took 36 min against its 18 min ETA from sonnet-high to worker at medium effort"), "{}", route.why);
    let under = Evidence { reopened: false, time: Some(crate::ledger::RunTime { minutes: 35, eta_min: 18 }) };
    assert_eq!(spinning(report, &[], &under).stop.climb, None, "under twice the ETA keeps the rung");
}

#[test]
fn the_south_lock_refused_twice_climbs_the_box_one_rung_at_the_same_refusal() {
    use super::spin::Signal;
    let held = Notice::Refused { branch: "n940-verify".into(), head: "abc".into(), verdict: "the south checkout is locked by another safe-push".into(), errors: None }.text();
    let route = super::tier::route(&spinning("Not done: the lane is still queued.\nStop: landing", &[held.clone(), held], &Evidence::default()), &super::tier::History::default(), &BTreeMap::new()).unwrap();
    assert_eq!(route.signal, Some(Signal::LandRefusal), "{}", route.why);
    assert!(route.why.contains("lane n940-verify, refused twice for the same reason: the south checkout is locked"), "{}", route.why);
}

#[test]
fn a_box_a_missed_check_reopened_climbs_one_rung_and_its_launch_record_names_the_signal_that_fired() {
    use super::spin::Signal;
    let missed = issue("north", 940, "- [ ] the check passes <!-- check n940@2026-11-10 missed -->", &[]);
    assert!(missed.reopened(None), "an unticked box whose check missed was reopened");
    assert!(missed.reopened(Some("the check passes")), "the box the launch staffs is the one that reopened");
    assert!(!missed.reopened(Some("another box")), "a miss on another box is no signal for this one");
    assert!(!issue("north", 940, "- [ ] a", &[]).reopened(None));
    let reopened = Evidence { reopened: true, time: None };
    let issue = spinning("Not done: the fix did not hold.\nStop: landing", &[], &reopened);
    assert_eq!(issue.stop.climb.as_ref().map(|climb| climb.signal), Some(Signal::Reopened));
    let route = super::tier::route(&issue, &super::tier::History::default(), &BTreeMap::new()).unwrap();
    let launched = record::Record { at: 10.0, item: "north#940".into(), host: "gex45".into(), account: "claude:c".into(), name: "n940-d0901".into(), cloud: false, launched: true, detail: String::new(), tier: Some(route.tier.name().to_owned()), category: Some(route.category.clone()), box_text: None, hand: false, signal: None, lift: None, effort_arm: None, effort: None, cell: None, cell_role: None };
    let entry = record::Record { signal: route.signal.map(|signal| signal.key().to_owned()), ..launched.clone() };
    let line = serde_json::to_string(&entry).unwrap();
    assert!(line.contains("\"signal\":\"reopened\""), "{line}");
    assert!(!serde_json::to_string(&launched).unwrap().contains("signal"), "a launch that did not climb records no signal");
}

/// north#956: north-956-d0754 was staffed on 2026-11-10 on a box whose own line reads `Needs: Tom` while the gate it names stayed closed as paused.
#[test]
fn a_box_whose_own_line_needs_tom_is_not_staffed_while_a_plain_box_beside_it_is() {
    let at = crate::foundation::clock::Timestamp::parse_loose("2026-11-10T07:54Z").unwrap().unix_seconds_f64();
    let (open, holds, planned, live, no_boxes) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new(), BTreeSet::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, box_holds: &no_boxes, box_tries: &no_boxes, live: &live, planned: &planned, backlog: None, at };
    let tom = "- [ ] Every transcript has a verified cold copy on the Storage Box. Needs: Tom (resuming the paused off-host push, gate 192)";
    let plain = "- [ ] the cache keeps the newest row";

    let both = issue("north", 956, &format!("{tom}\n{plain}"), &[]);
    assert_eq!(both.takeable(at).collect::<Vec<_>>(), [plain]);
    assert_eq!(
        issues::readiness(&issue("north", 956, tom, &[]), &world),
        Err("only open box waits until Tom answers it: its own `Needs: Tom` line hands it to him".into())
    );
}
