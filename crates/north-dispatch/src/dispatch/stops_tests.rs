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

    let fleet = tempfile::tempdir().unwrap();
    let archived = fleet.path().join(".archive/north-714-d2130-1794000770958");
    std::fs::create_dir_all(&archived).unwrap();
    let note = serde_json::json!({"why": "its parent took its final report", "at_ms": 1_794_000_770_958_u64, "last": "Not done: no box of north#714 is ticked, because the E1 fidelity measurement needs Tom's laptop.\n\nStop: host laptop"});
    std::fs::write(archived.join("retired.json"), note.to_string()).unwrap();
    let mut boxes = [issue("north", 714, "- [ ] E1 fidelity on the laptop's transcripts", &[])];
    let mut notes = Vec::new();
    let gone = |_: &record::Record| Err::<serde_json::Value, String>("error: no session named north-714-d2130".into());
    super::survey::stopped(&[launch("north#714", "north-714-d2130", 1_794_000_658.0, true)], &mut boxes, gone, |name| crate::fleet::records::retired_report(fleet.path(), name), &mut notes);
    assert_eq!((boxes[0].stop.host.as_deref(), notes.len()), (Some("laptop"), 0), "the reaper retired the session before dispatch read it, and its archived report still holds the box off gex45: {notes:?}");

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
        issue.needs = issue.box_needs(&hosts, d0216);
        let brief = super::brief::brief(&issue, "dispatch", &route, &super::grounds::Grounds::default());
        hosts.iter().filter(|host| *host != "nexus" && crate::pacing::placement::gate::lacks(&cfg, host, &brief).is_none()).cloned().collect::<Vec<_>>()
    };
    assert_eq!(placed(733, "- [ ] On the laptop (owner: a laptop-placed session; gex45 has no shell there), exercise a proxy take-over: `niri msg -j windows` shows a ghostty window titled `north proxy`"), ["whiterabbit"], "north-733-d0216 went to gex45, which has no niri desktop");
    assert_eq!(placed(731, "- [ ] Three consecutive landings go live on all four hosts within 2 minutes each, with a dirty checkout planted during one of them that does not stop the runtime."), ["whiterabbit"], "north-731-d0216 went to gex45, which reaches no other host");
    let goals = "- [ ] makgora's frame goals read the current main: a scheduled job re-runs `bun wisp perf playable-bot-four --samples` (76 s)\n- [ ] makgora's and wisp's goals that stay unmeasured get an oracle or a stated reason\n- [ ] The experiment decisions-never-stop-work is reviewed on 2026-11-23 (registered in north ce12b5de3)";
    assert_eq!(placed(641, goals), ["gex45", "whiterabbit"], "north-641-d0216 went to ccx33, which holds no makgora or wisp checkout");
    assert_eq!(placed(630, "- [ ] `north ops check` on Nexus shows launch_failures within 5 over 6 h for a full day after both land. (not before 2026-11-07T00:00Z) The fixes landed as 8f2fde4c7 and 1eb741624 on 2026-11-05."), ["whiterabbit"], "north-630-d0216 went to ccx33, which cannot reach nexus");
    let measured = issue("north", 706, "- [ ] over 24 h no run is attributed host-caused (not before 2026-11-08T00:00Z)\n- [ ] over 6 h no worker run's report names only a measurement that has not come due (not before 2026-11-08T00:00Z)\n- [ ] Clean-up: scaffolding removed.", &[]);
    assert_eq!(issues::readiness(&measured, &world(d0216)), Err("every open box waits, the first until 2026-11-08T00:00Z".into()), "north-706-d0216 was staffed while every box's measurement was not yet due");
    let reported = Issue { stop: super::stops::stop(&session("Not done: every open box is a measurement whose window has not elapsed.\nStop: wait until 2026-11-08T00:00Z"), &ran), ..issue("north", 706, "- [ ] over 24 h no run is attributed host-caused", &[]) };
    assert_eq!(issues::readiness(&reported, &world(d0216)), Err("its last worker stopped to wait until 2026-11-08T00:00Z".into()));
    let gone = |state: &str, body: &str| super::survey::github::gone(&serde_json::json!({"state": state, "body": body}));
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

#[test]
fn north_725_d0411_is_held_off_gex45_by_the_host_stops_of_both_earlier_launches_though_the_newest_ended_on_a_reply_to_the_lander() {
    let session = |said: &[&str]| serde_json::json!({
        "entries": said.iter().map(|text| serde_json::json!({"kind": "assistant", "text": text})).collect::<Vec<_>>(),
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let launch = |name: &str, at: f64| record::Record { at, item: "north#725".into(), host: "gex45".into(), account: "claude:c".into(), name: name.into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, hand: false, effort_arm: None, effort: None };
    let body = "- [ ] the issue holds the per-host byte table with under 5% unaccounted.\n- [ ] gex45 sits under its low watermark after one cleanup pass (df output), and a replay of the 99d4ba44d1 condition trims caches before any land slot.\n- [x] the change is on origin/main";
    let d0306 = session(&["Not done: the ledger fix landed, but two of the three Done-when boxes on north#725 are still open. ccx33 and the laptop were not measured.\n\nStop: host ccx33"]);
    let d0352 = session(&[
        "Not done: the laptop row of the per-host byte table needs `north capacity disk` run on the laptop itself.\n\nStop: host laptop",
        "The lander's message confirms the landing I already reported: `disk-reflink` is on origin/main as e17ec4208. Nothing changes in my report.",
    ]);
    let directory = tempfile::tempdir().unwrap();
    let pass = |records: &[record::Record], at: f64| {
        let mut boxes = [issue("north", 725, body, &[])];
        let show = |ran: &record::Record| Ok::<serde_json::Value, String>(if ran.name == "north-725-d0306" { d0306.clone() } else { d0352.clone() });
        super::survey::stopped(records, &mut boxes, show, |_| None, &mut Vec::new());
        let launches = super::stops::last_launches(records);
        let learned = super::host_needs::learn(directory.path(), &boxes, |issue| launches.get(&issue.reference()).map(|launch| launch.name.clone()), at).unwrap();
        let [mut issue] = boxes;
        issue.needs.extend(super::host_needs::holding(&learned, &issue, at));
        issue
    };
    let (d0306_at, d0352_at, d0411_at) = (1_794_027_996.0, 1_794_030_733.0, 1_794_033_106.0);
    pass(&[launch("north-725-d0306", d0306_at)], d0306_at + 2100.0);
    let issue = pass(&[launch("north-725-d0306", d0306_at), launch("north-725-d0352", d0352_at)], d0411_at);
    assert_eq!(issue.stop.host.as_deref(), Some("laptop"), "north-725-d0352's report is the Not done message, not its later reply to the lander");
    let route = super::tier::Route { tier: Tier::Sonnet, category: "feature".into(), effort: None, effort_arm: false, follows: None, why: String::new() };
    let brief = super::brief::brief(&issue, "dispatch", &route, &super::grounds::Grounds::default());
    assert!(brief.lines().any(|line| line == "Needs: ccx33, laptop"), "north-725-d0306's Stop: host ccx33 still holds, though a later launch ran since:\n{brief}");
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
    assert_eq!(lacks("gex45").as_deref(), Some("laptop"), "north-725-d0411 went to gex45 and stopped on Stop: host laptop");
    assert_eq!((lacks("ccx33").as_deref(), lacks("cpx62").as_deref(), lacks("whiterabbit")), (Some("laptop"), Some("laptop"), None));
    let ticked = Issue { body: body.replace("- [ ] the issue holds", "- [x] the issue holds").replace("- [ ] gex45 sits", "- [x] gex45 sits"), ..issue };
    assert!(super::host_needs::holding(&super::host_needs::read(directory.path()), &ticked, d0411_at).is_empty(), "a need goes once every box it was learned for is ticked");
}

#[test]
fn a_hand_launch_on_north_759_reads_the_nexus_stop_of_n_claim_reland_and_gex45_is_refused_while_an_item_with_no_need_places_as_before() {
    let ran = record::Record { at: 1_794_040_000.0, item: "north#759".into(), host: "gex45".into(), account: "claude:c".into(), name: "n-claim-reland".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, hand: true, effort_arm: None, effort: None };
    let reported = serde_json::json!({
        "entries": [{"kind": "assistant", "text": "Not done: the claim reland needs the ledger on nexus.\n\nStop: host nexus"}],
        "turns": [{"exit": "completed", "ended_ms": 1_000_000}]
    });
    let at = 1_794_041_000.0;
    let directory = tempfile::tempdir().unwrap();
    let body = "- [ ] the claim lands on the ledger host and a replay shows it held";
    let mut boxes = [issue("north", 759, body, &[])];
    super::survey::stopped(std::slice::from_ref(&ran), &mut boxes, |_| Ok::<serde_json::Value, String>(reported.clone()), |_| None, &mut Vec::new());
    let launches = super::stops::last_launches(std::slice::from_ref(&ran));
    let learned = super::host_needs::learn(directory.path(), &boxes, |issue| launches.get(&issue.reference()).map(|launch| launch.name.clone()), at).unwrap();
    let hosts: Vec<String> = ["gex45", "nexus", "whiterabbit"].map(str::to_owned).to_vec();
    let [claimed] = boxes;
    let needs = super::host_needs::of(&claimed, &hosts, &learned, at);
    assert_eq!(needs, ["nexus"], "the hand launch's own Stop: host is learned like a dispatched one's");

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
    let none = super::host_needs::of(&plain, &hosts, &[], at);
    assert!(none.is_empty());
    assert_eq!(crate::pacing::placement::needs::with_needs(unneeding, &none), unneeding);
    assert_eq!(crate::pacing::placement::gate::lacks(&cfg, "gex45", unneeding), None);

    let named = |text: &str| crate::pacing::placement::needs::in_box(text, "north", &hosts, &[]);
    assert_eq!(named("- [ ] The first live `goals red (N)` line reaches the proxy through the fleet tick after the runtime carrying `probes.settle.goals` is promoted (observed in a tick's output)"), ["fleet"], "north#641's first box runs on the host that runs the tick");
    assert_eq!(named("- [ ] the laptop rebuild and the timer seen running there"), ["laptop"], "north-641-d0441 stopped on Stop: host laptop");
    assert!(named("- [ ] the laptop field of the Host struct is read from config and a test pins it").is_empty(), "a code box that mentions a laptop field is not the laptop's");
}
