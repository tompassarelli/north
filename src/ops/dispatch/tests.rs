use super::issues::{self, Issue, IssueRef, World};
use super::plan::Cloud;
use super::slots::{Account, Host, Pools};
use super::survey::Survey;
use super::{alert, decide, record};
use std::collections::{BTreeMap, BTreeSet};

const CONFIG: &str = r#"
[hosts.nexus]
resident_cap = 6
[hosts.gex45]
resident_cap = 24
ssh = "tom@gex45"
serves = ["native:smashcraft", "warcraft:smashcraft"]
[hosts.whiterabbit]
resident_cap = 8
serves = ["native", "warcraft", "sign-ins", "laptop"]
[placement]
order = ["gex45", "anthropic-cloud", "nexus", "laptop"]
"#;

fn host(name: &str, cap: i64, running: Result<i64, &str>) -> Host {
    Host { name: name.into(), cap: Some(cap), running: running.map_err(str::to_owned) }
}

fn account(name: &str, allowed: i64, working: i64, hosts: &[&str], headroom: f64) -> Account {
    Account { name: name.into(), allowed: Some(allowed), working, hosts: hosts.iter().map(|host| (*host).to_owned()).collect(), headroom, refusal: None }
}

fn issue(repo: &str, number: u64, body: &str, labels: &[&str]) -> Issue {
    Issue { repo: repo.into(), number, title: format!("box {number}"), body: body.into(), labels: labels.iter().map(|label| (*label).to_owned()).collect(), open_blockers: Vec::new() }
}

fn survey(issues: Vec<Issue>, cloud: Cloud, hosts: Vec<Host>, accounts: Vec<Account>) -> Survey {
    let open = issues.iter().map(Issue::reference).collect();
    Survey {
        here: "nexus".into(),
        cfg: CONFIG.parse().unwrap(),
        hosts,
        accounts,
        issues,
        open,
        holds: BTreeMap::new(),
        cloud,
        laptop: Some("whiterabbit".into()),
        notes: Vec::new(),
    }
}

fn fleet() -> (Vec<Host>, Vec<Account>) {
    let hosts = vec![host("gex45", 24, Ok(22)), host("nexus", 6, Ok(1)), host("whiterabbit", 8, Err("laptop unavailable"))];
    let accounts = vec![account("claude:a", 2, 1, &["nexus", "whiterabbit"], 70.0), account("claude:b", 3, 0, &["gex45"], 40.0), account("claude:c", 1, 1, &["gex45"], 90.0)];
    (hosts, accounts)
}

#[test]
fn a_pair_has_the_smaller_of_its_hosts_free_cap_and_its_accounts_free_allowance() {
    let (hosts, accounts) = fleet();
    let mut pools = Pools::new(&hosts, &accounts);
    assert_eq!(hosts[0].free(), Ok(2));
    assert_eq!(accounts[1].free(), Ok(3));
    assert_eq!(pools.pair("gex45", "claude:b"), 2, "min(24 - 22, 3 - 0)");
    assert_eq!(pools.pair("gex45", "claude:c"), 0, "c runs its one allowed worker already");
    assert_eq!(pools.pair("nexus", "claude:a"), 1, "min(6 - 1, 2 - 1)");
    assert_eq!(pools.pair("nexus", "claude:b"), 0, "b is not signed in on nexus");
    assert_eq!(pools.pair("whiterabbit", "claude:a"), 0, "an unavailable laptop takes nothing");
    assert_eq!(pools.total(), 3);
    pools.take("gex45", "claude:b");
    pools.take("gex45", "claude:b");
    assert_eq!((pools.pair("gex45", "claude:b"), pools.total()), (0, 1), "two launches fill gex45's two free slots");
    let full = host("gex45", 4, Ok(9));
    assert_eq!(full.free(), Ok(0), "running past the cap leaves none, never a negative slot");
    let out = Account { refusal: Some("claude:b unavailable: 5h 96%".into()), ..account("claude:b", 3, 0, &["gex45"], 0.0) };
    assert_eq!(out.free(), Err("claude:b unavailable: 5h 96%".into()));
    let unsampled = Account { allowed: None, ..account("codex", 0, 0, &["nexus"], 0.0) };
    assert!(unsampled.free().is_err());
}

#[test]
fn a_ready_box_is_open_unticked_unheld_not_toms_and_after_its_closed_predecessors() {
    let plan = issue("smashcraft", 1, "## Order of work\n1. #10 first\n2. wisp#3 then\n3. #12 last\n## After\n- [ ] ship", &["roadmap"]);
    let issues = vec![
        plan.clone(),
        issue("smashcraft", 10, "- [ ] a", &[]),
        issue("smashcraft", 11, "- [x] done", &[]),
        issue("smashcraft", 12, "- [ ] c", &[]),
        issue("smashcraft", 13, "- [ ] d", &["evidence:tom"]),
        issue("smashcraft", 14, "- [ ] e\nBlocked by #10", &[]),
        issue("smashcraft", 15, "- [ ] f\nFollows: wisp#9", &[]),
        issue("smashcraft", 16, "- [ ] g\nFollows: the landing of the relay port", &[]),
        issue("smashcraft", 17, "- [ ] h", &[]),
        Issue { open_blockers: vec![IssueRef::new("north", 190)], ..issue("smashcraft", 18, "- [ ] i", &[]) },
        issue("wisp", 3, "- [ ] j", &[]),
    ];
    let open: BTreeSet<IssueRef> = issues.iter().map(Issue::reference).chain([IssueRef::new("north", 190)]).collect();
    let holds = BTreeMap::from([(IssueRef::new("smashcraft", 17), "fleet session sc-17-a on gex45".to_owned())]);
    let planned = issues::planned_predecessors(&issues);
    let world = World { open: &open, holds: &holds, planned: &planned };
    let verdict = |number: usize| issues::readiness(&issues[number], &world);
    assert_eq!(verdict(0), Err("labelled roadmap: its child issues are the boxes".into()));
    assert_eq!(verdict(1), Ok(()), "the first planned box is ready");
    assert_eq!(verdict(2), Err("no unticked Done-when box".into()));
    assert_eq!(verdict(3), Err("waits on open smashcraft#10, wisp#3".into()), "plan order holds the later box");
    assert_eq!(verdict(4), Err("labelled evidence:tom, so Tom decides".into()));
    assert_eq!(verdict(5), Err("waits on open smashcraft#10".into()));
    assert_eq!(verdict(6), Ok(()), "a closed Follows: predecessor releases the box");
    assert!(verdict(7).unwrap_err().contains("follows-release starts it"));
    assert_eq!(verdict(8), Err("held by fleet session sc-17-a on gex45".into()));
    assert_eq!(verdict(9), Err("waits on open north#190".into()));
    assert_eq!(verdict(10), Err("waits on open smashcraft#10".into()), "a plan in another repository orders it too");
}

fn placed(survey: &Survey) -> Vec<(String, String, String, bool, Option<String>)> {
    decide(survey, "test").launches.into_iter().map(|launch| (launch.item.to_string(), launch.host, launch.account, launch.courier, launch.local)).collect()
}

#[test]
fn code_only_boxes_fall_back_to_a_host_with_a_free_slot_when_the_cloud_lane_is_unreachable_or_its_courier_slot_is_full() {
    let (hosts, accounts) = fleet();
    let boxes = vec![issue("smashcraft", 20, "- [ ] a", &[]), issue("smashcraft", 21, "- [ ] b", &[]), issue("smashcraft", 22, "- [ ] c", &[]), issue("smashcraft", 23, "- [ ] d", &[])];
    let unreachable = survey(boxes.clone(), Cloud::Unreachable("the credit login is on nexus".into()), hosts.clone(), accounts.clone());
    let why = Some("north dispatch: cloud lane unreachable from nexus: the credit login is on nexus".to_owned());
    assert_eq!(
        placed(&unreachable),
        [
            ("smashcraft#20".into(), "gex45".into(), "claude:b".into(), false, why.clone()),
            ("smashcraft#21".into(), "gex45".into(), "claude:b".into(), false, why.clone()),
            ("smashcraft#22".into(), "nexus".into(), "claude:a".into(), false, why.clone()),
        ]
    );
    let outcome = decide(&unreachable, "test");
    assert_eq!((outcome.ready, outcome.free), (4, 3));
    let (_, last) = outcome.skipped.last().unwrap();
    assert!(last.contains("gex45 has no free slot") && last.contains("whiterabbit takes nothing: laptop unavailable"), "{last}");

    let reachable = survey(boxes.clone(), Cloud::Reachable("cloud credit: unread".into()), hosts.clone(), accounts.clone());
    let full = Some("north dispatch: cloud lane open (cloud credit: unread) but nexus has no free slot for its courier".to_owned());
    assert_eq!(
        placed(&reachable),
        [
            ("smashcraft#20".into(), "nexus".into(), "claude:a".into(), true, Some("north dispatch: courier for the cloud lane (cloud credit: unread)".to_owned())),
            ("smashcraft#21".into(), "gex45".into(), "claude:b".into(), false, full.clone()),
            ("smashcraft#22".into(), "gex45".into(), "claude:b".into(), false, full),
        ]
    );

    let closed = survey(boxes, Cloud::Closed("cloud credit: expired".into()), hosts, accounts);
    assert!(placed(&closed).iter().all(|(_, _, _, courier, local)| !courier && local.is_none()));
}

#[test]
fn native_boxes_go_only_where_their_needs_are_served() {
    let (mut hosts, accounts) = fleet();
    let native = issue("smashcraft", 30, "- [ ] a", &["needs:native-pair"]);
    let wisp_native = issue("wisp", 31, "- [ ] b", &["needs:native-single"]);
    let launched = placed(&survey(vec![native.clone(), wisp_native.clone()], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()));
    assert_eq!(launched.iter().map(|row| (row.0.as_str(), row.1.as_str())).collect::<Vec<_>>(), [("smashcraft#30", "gex45")]);
    hosts[2].running = Ok(0);
    let launched = placed(&survey(vec![wisp_native], Cloud::Closed(String::new()), hosts, accounts));
    assert_eq!(launched.iter().map(|row| (row.0.as_str(), row.1.as_str())).collect::<Vec<_>>(), [("wisp#31", "whiterabbit")], "the open laptop serves native wisp");
}

#[test]
fn tom_hears_once_only_when_ready_work_and_free_slots_coexist_and_nothing_launched() {
    let (hosts, accounts) = fleet();
    let stuck = survey(vec![issue("wisp", 40, "- [ ] a", &["needs:native-single"])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone());
    let outcome = decide(&stuck, "test");
    let line = alert(&outcome, &[], "nexus").unwrap();
    assert!(line.starts_with("north dispatch on nexus: 1 ready, 3 free slots, none launched: wisp#40 ready, not placed: gex45 does not serve native"), "{line}");
    let busy = survey(vec![issue("smashcraft", 41, "- [ ] a", &[])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone());
    let launched = decide(&busy, "test");
    assert_eq!(alert(&launched, &[], "nexus"), None, "a launch went out");
    assert!(alert(&launched, &["smashcraft#41 on gex45: refused".into()], "nexus").unwrap().ends_with("none launched: smashcraft#41 on gex45: refused"));
    let idle = survey(Vec::new(), Cloud::Closed(String::new()), hosts, accounts);
    assert_eq!(alert(&decide(&idle, "test"), &[], "nexus"), None, "no ready work is no alert");
}

#[test]
fn the_record_holds_a_fresh_launch_and_hands_a_twice_tried_item_to_the_proxy() {
    let entry = |item: &str, at: f64| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: "sc-1-d0000".into(), courier: false, launched: true, detail: String::new() };
    let now = 100_000.0;
    let records = vec![entry("smashcraft#1", now - 600.0), entry("smashcraft#2", now - 4.0 * 3600.0), entry("wisp#3", now - 5.0 * 3600.0), entry("wisp#3", now - 3.0 * 3600.0)];
    let holds = record::holds(&records, now);
    assert!(holds[&IssueRef::new("smashcraft", 1)].starts_with("north dispatch launch sc-1-d0000 on gex45 10 min ago"));
    assert!(!holds.contains_key(&IssueRef::new("smashcraft", 2)), "an old single launch whose session ended frees the box");
    assert!(holds[&IssueRef::new("wisp", 3)].contains("2 tries in 24 h"));
    let open = BTreeSet::from([IssueRef::new("smashcraft", 1)]);
    assert_eq!(record::recent_on(&records, "gex45", &open, now), 1);
}

#[test]
fn running_sessions_hold_their_task_or_their_named_issue() {
    let listing = serde_json::json!([
        {"name": "sc-256-ending", "provider": "claude", "account": "c", "status": "working", "task_ref": null, "machine": null},
        {"name": "fixer", "provider": "claude", "account": "b", "status": "idle", "task_ref": "tompassarelli/wisp#21", "machine": "gex45"},
        {"name": "sc-9-old", "provider": "claude", "account": "b", "status": "parked", "task_ref": null, "machine": null},
        {"name": "proxy", "provider": "codex", "account": "pooled", "status": "starting", "task_ref": null, "machine": null}
    ]);
    let sessions = super::survey::running_sessions(&listing);
    let items: Vec<Option<String>> = sessions.iter().map(|session| session.item.as_ref().map(ToString::to_string)).collect();
    assert_eq!(items, [Some("smashcraft#256".into()), Some("wisp#21".into()), None]);
    assert_eq!(sessions[2].account, "codex:pooled");
}

#[test]
fn a_threads_claim_on_any_hosts_ledger_holds_its_issue() {
    let listing = "item  owner  clock  eta  flag\nwisp#21  sc-21-a  12m  45m  -\nsmashcraft#9  w4  3m  -  OVERDUE\n";
    let rows = super::survey::claim_rows(listing, "gex45");
    assert_eq!(rows, [(IssueRef::new("wisp", 21), "threads claim by sc-21-a on gex45".to_owned()), (IssueRef::new("smashcraft", 9), "threads claim by w4 on gex45".to_owned())]);
    assert!(super::survey::claim_rows("item  owner  clock  eta  flag\n", "nexus").is_empty());
}

#[test]
fn an_account_placement_never_judged_gets_no_slot() {
    let control = serde_json::json!({"at": 1000.0, "allowed_workers": 2});
    let judged = crate::ops::pacing::placement::gate::Verdict { account: "claude:b".into(), available: true, headroom: 40.0, line: "claude:b ok".into(), owners: vec!["gex45".into()] };
    assert_eq!(super::survey::refusal("claude:b", &control, Some(&judged), 1060.0), None);
    let unjudged = super::survey::refusal("codex", &control, None, 1060.0).unwrap();
    assert!(unjudged.contains("codex has no placement verdict"), "{unjudged}");
    assert!(super::survey::refusal("claude:b", &control, Some(&judged), 1000.0 + 16.0 * 60.0).unwrap().contains("sampled 16 min ago"));
}

#[test]
fn the_laptops_copied_fleet_list_holds_its_sessions_issues_even_when_old() {
    let directory = tempfile::tempdir().unwrap();
    let mut notes = Vec::new();
    assert!(super::survey::mirrored(directory.path(), "whiterabbit", 0.0, &mut notes).is_empty());
    assert!(notes[0].starts_with("whiterabbit fleet list unread"), "{notes:?}");
    let listing = serde_json::json!([{"name": "fixer", "provider": "claude", "account": "a", "status": "working", "task_ref": "tompassarelli/smashcraft#350", "machine": null}]);
    std::fs::write(directory.path().join("fleet-whiterabbit.json"), listing.to_string()).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let mut notes = Vec::new();
    let old = super::survey::mirrored(directory.path(), "whiterabbit", now + 3600.0, &mut notes);
    assert_eq!(old.iter().map(|session| session.item.clone()).collect::<Vec<_>>(), [Some(IssueRef::new("smashcraft", 350))]);
    assert_eq!(old[0].account, "claude:a");
    assert!(notes[0].starts_with("whiterabbit fleet list copied 60 min ago"), "{notes:?}");
}

#[test]
fn an_idle_session_holds_its_issue_but_takes_no_worker_slot() {
    let listing = serde_json::json!([
        {"name": "sc-340", "provider": "claude", "account": "c", "status": "idle", "task_ref": null, "machine": null},
        {"name": "sc-341", "provider": "claude", "account": "c", "status": "working", "task_ref": null, "machine": null},
        {"name": "sc-342", "provider": "claude", "account": "b", "status": "starting", "task_ref": null, "machine": null}
    ]);
    let sessions: Vec<_> = super::survey::running_sessions(&listing).into_iter().map(|session| (session, "gex45".to_owned())).collect();
    let (per_account, _, holds) = super::survey::tally(&sessions);
    assert_eq!(per_account, BTreeMap::from([("claude:b".to_owned(), 1), ("claude:c".to_owned(), 1)]));
    assert_eq!(holds[&IssueRef::new("smashcraft", 340)], "fleet session sc-340 on gex45");
}
