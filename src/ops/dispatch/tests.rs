use super::issues::{self, Issue, IssueRef, World};
use super::plan::Cloud;
use super::slots::{Account, Host, Pools, Room};
use super::survey::Survey;
use super::{alert, decide, record};
use crate::fleet::model::{Account as Login, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::ops::pacing::placement::gate;
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
[accounts.claude]
profiles = { a = { nexus = {}, whiterabbit = {} }, b = { gex45 = {} } }
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
[tiers."claude:sonnet-high"]
model = "claude-sonnet-5-5"
effort = "high"
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
"#;

fn host(name: &str, cap: i64, running: Result<i64, &str>) -> Host {
    Host { name: name.into(), cap: Some(cap), running: running.map_err(str::to_owned), measure: None }
}

fn account(name: &str, hosts: &[&str], headroom: f64) -> Account {
    sized(name, hosts, 1.0, Room { five_hour: headroom, week: headroom, week_reset_h: 100.0 })
}

fn sized(name: &str, hosts: &[&str], plan: f64, room: Room) -> Account {
    Account { name: name.into(), hosts: hosts.iter().map(|host| (*host).to_owned()).collect(), plan, room, spending: false, refusal: None }
}

pub(super) fn issue(repo: &str, number: u64, body: &str, labels: &[&str]) -> Issue {
    Issue { repo: repo.into(), number, title: format!("box {number}"), body: body.into(), labels: labels.iter().map(|label| (*label).to_owned()).collect(), ..Issue::default() }
}

pub(super) fn survey(issues: Vec<Issue>, cloud: Cloud, hosts: Vec<Host>, accounts: Vec<Account>) -> Survey {
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
        cloud_running: 0,
        notes: Vec::new(),
        frozen: Vec::new(),
        history: super::tier::History::default(),
        handoffs: super::handoff::Arms::default(),
        ledger: BTreeMap::new(),
        at: 0.0,
    }
}

pub(super) fn fleet() -> (Vec<Host>, Vec<Account>) {
    let hosts = vec![host("gex45", 24, Ok(22)), host("nexus", 6, Ok(5)), host("whiterabbit", 8, Err("laptop unavailable"))];
    let out = Account { refusal: Some("claude:c unavailable: exhausted: 5h 96% >= 95%".into()), ..account("claude:c", &["gex45"], 90.0) };
    let accounts = vec![account("claude:a", &["nexus", "whiterabbit"], 70.0), account("claude:b", &["gex45"], 40.0), out];
    (hosts, accounts)
}

#[test]
fn an_open_account_takes_every_free_slot_of_its_hosts_and_an_account_the_out_rule_holds_takes_none() {
    let (hosts, accounts) = fleet();
    let mut pools = Pools::new(&hosts, &accounts);
    assert_eq!(hosts[0].free(), Ok(2));
    assert_eq!(pools.pair("gex45", "claude:b"), 2, "no pace allowance caps an open account");
    assert_eq!(pools.pair("gex45", "claude:c"), 0, "the out-rule holds c");
    assert_eq!(pools.pair("nexus", "claude:a"), 1);
    assert_eq!(pools.pair("nexus", "claude:b"), 0, "b is not signed in on nexus");
    assert_eq!(pools.pair("whiterabbit", "claude:a"), 0, "an unavailable laptop takes nothing");
    assert_eq!(pools.total(), 3);
    pools.take("gex45");
    pools.take("gex45");
    assert_eq!((pools.pair("gex45", "claude:b"), pools.total()), (0, 1), "two launches fill gex45's two free slots");
    let full = host("gex45", 4, Ok(9));
    assert_eq!(full.free(), Ok(0), "running past the cap leaves none, never a negative slot");
    assert_eq!(accounts[2].open(), Err("claude:c unavailable: exhausted: 5h 96% >= 95%".into()));
}

#[test]
fn only_the_out_rule_refuses_an_account_never_its_week_projection() {
    let window = |name: &str, used: f64, burn: f64, projected: f64| serde_json::json!({"window": name, "used_pct": used, "burn_pct_h": burn, "projected_pct": projected});
    let usage = |five: serde_json::Value, week: serde_json::Value| serde_json::json!({"windows": [five, week]});
    let control = serde_json::json!({"at": 1000.0, "allowed_workers": 1, "held": true, "over": true});
    let refused = |usage: serde_json::Value| super::survey::refusal("claude:b", &control, Some(&gate::verdict("claude:b", &usage)), 1060.0);
    assert_eq!(refused(usage(window("5h", 53.0, 5.9, 61.0), window("week", 61.0, 1.0, 191.0))), None, "a week projected to 191% over 133 h holds nothing");
    assert!(refused(usage(window("5h", 96.0, 1.0, 99.0), window("week", 10.0, 1.0, 20.0))).unwrap().contains("5h 96% >= 95%"));
    assert!(refused(usage(window("5h", 10.0, 60.0, 99.0), window("week", 10.0, 1.0, 20.0))).unwrap().contains("burning"));
    assert!(refused(usage(window("5h", 10.0, 1.0, 20.0), window("week", 100.0, 0.0, 100.0))).unwrap().contains("week 100%"));
}

#[test]
fn an_account_runs_only_where_it_is_signed_in_and_here_only_with_a_login_of_its_own() {
    let cfg = CONFIG.parse().unwrap();
    let login = |label: &str| (Login { provider: ProviderName::new("claude"), label: label.into(), home: format!("/profiles/claude-{label}").into(), token_file: None }, None);
    let without_a = Roster::distinct([login("b")], &Declared::default());
    assert_eq!(super::survey::signed_in(&cfg, &without_a, "claude:a", "nexus"), ["whiterabbit"], "nexus has no login for a");
    let with_a = Roster::distinct([login("a")], &Declared::default());
    assert_eq!(super::survey::signed_in(&cfg, &with_a, "claude:a", "nexus"), ["nexus", "whiterabbit"]);
    assert_eq!(super::survey::signed_in(&cfg, &without_a, "claude:b", "nexus"), ["gex45"]);
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
    let world = World { open: &open, holds: &holds, planned: &planned, at: 0.0 };
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

#[test]
fn a_box_whose_only_open_line_waits_on_a_future_time_is_skipped_until_that_time_passes() {
    let gated = issue("wisp", 3, "- [x] wired\n- [ ] Autoland refusals over the 7-day window (not before 2026-10-26T05:13Z)", &[]);
    let mixed = issue("wisp", 4, "- [ ] count (not before 2026-10-26T05:13Z)\n- [ ] measurable now", &[]);
    let both = issue("wisp", 5, "- [ ] a (not before 2026-10-27T00:00Z)\n- [ ] b (not before 2026-10-26T05:13Z)", &[]);
    let (open, holds, planned) = (BTreeSet::new(), BTreeMap::new(), BTreeMap::new());
    let at = |at: f64| World { open: &open, holds: &holds, planned: &planned, at };
    let marked = 1_792_991_580.0;
    assert_eq!(issues::readiness(&gated, &at(marked - 60.0)), Err("only open box waits until 2026-10-26T05:13Z".into()));
    assert_eq!(issues::readiness(&gated, &at(marked)), Ok(()), "the box is staffed once its time has come");
    assert_eq!(issues::readiness(&mixed, &at(marked - 60.0)), Ok(()), "an ungated open box is work now");
    assert_eq!(issues::readiness(&both, &at(marked - 60.0)), Err("every open box waits, the first until 2026-10-26T05:13Z".into()));
    assert_eq!(issues::readiness(&both, &at(marked)), Ok(()));
    let (hosts, accounts) = fleet();
    let staffed = |at: f64| placed(&Survey { at, ..survey(vec![gated.clone()], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()) }).len();
    assert_eq!((staffed(marked - 60.0), staffed(marked + 60.0)), (0, 1));
}

fn placed(survey: &Survey) -> Vec<(String, String, String, bool, Option<String>)> {
    decide(survey, "test").launches.into_iter().map(|launch| (launch.item.to_string(), launch.host, launch.account, launch.cloud, launch.local)).collect()
}

fn chosen(hosts: Vec<Host>, accounts: Vec<Account>, boxes: u64) -> Vec<(String, Option<String>)> {
    let boxes = (0..boxes).map(|number| issue("smashcraft", 20 + number, "- [ ] a", &[])).collect();
    let unreachable = survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts);
    decide(&unreachable, "test").launches.into_iter().map(|launch| (launch.account, launch.pin)).collect()
}

fn accounts_of(chosen: &[(String, Option<String>)]) -> Vec<&str> {
    chosen.iter().map(|(account, _)| account.as_str()).collect()
}

#[test]
fn an_account_with_a_live_spend_target_takes_the_slot_before_any_whose_room_ranks_higher() {
    let spending = Account { spending: true, ..sized("claude:c", &["gex45"], 1.0, Room { five_hour: 40.0, week: 9.0, week_reset_h: 70.0 }) };
    let accounts = vec![sized("claude:a", &["gex45"], 4.0, Room { five_hour: 47.0, week: 13.0, week_reset_h: 60.0 }), spending];
    assert_eq!(chosen(vec![host("gex45", 24, Ok(22))], accounts, 2), [("claude:c".into(), None), ("claude:c".into(), None)]);
}

#[test]
fn plan_size_puts_accounts_on_one_scale_so_a_max_20x_quarter_outranks_a_max_5x_half() {
    let cfg: toml_edit::DocumentMut = "[accounts.claude]\nplans = { a = \"max-20x\", c = \"max-5x\", p = \"pro\" }".parse().unwrap();
    let plans: Vec<Result<f64, String>> = ["claude:a", "claude:c", "claude:p"].iter().map(|name| super::survey::plan(&cfg, name)).collect();
    assert_eq!(plans, [Ok(4.0), Ok(1.0), Ok(0.2)]);
    assert!(super::survey::plan(&cfg, "claude:b").is_err(), "an account with no plan is named in a note");
    let room = |week: f64| Room { five_hour: 50.0, week, week_reset_h: 100.0 };
    let accounts = vec![sized("claude:a", &["gex45"], 4.0, room(25.0)), sized("claude:c", &["gex45"], 1.0, room(50.0))];
    assert_eq!(accounts_of(&chosen(vec![host("gex45", 24, Ok(23))], accounts, 1)), ["claude:a"], "a quarter of Max 20x is twice half of Max 5x");
}

#[test]
fn a_week_that_resets_sooner_is_spent_first_and_five_hour_room_only_breaks_ties() {
    let at = crate::pace::target::seconds("2026-10-27T00:00:00Z").unwrap();
    let usage = |five: f64, week: f64, resets: &str| serde_json::json!({"windows": [{"window": "5h", "used_pct": five}, {"window": "week", "used_pct": week, "resets_at": resets, "reset_h": 1.0}]});
    let later = super::survey::room(&usage(10.0, 50.0, "2026-10-31T04:00:00Z"), at);
    let sooner = super::survey::room(&usage(80.0, 70.0, "2026-10-27T20:00:00Z"), at);
    assert_eq!(sooner, Room { five_hour: 15.0, week: 30.0, week_reset_h: 20.0 }, "resets_at, not the sampled reset_h, dates the loss");
    let accounts = vec![sized("claude:a", &["gex45"], 1.0, later), sized("claude:b", &["gex45"], 1.0, sooner)];
    assert_eq!(accounts_of(&chosen(vec![host("gex45", 24, Ok(23))], accounts, 1)), ["claude:b"], "30 points lost in 20 h beat 50 points lost in 100 h");
    let tied = |five: f64| Room { five_hour: five, week: 30.0, week_reset_h: 20.0 };
    let accounts = vec![sized("claude:a", &["gex45"], 1.0, tied(15.0)), sized("claude:b", &["gex45"], 1.0, tied(40.0))];
    assert_eq!(accounts_of(&chosen(vec![host("gex45", 24, Ok(23))], accounts, 1)), ["claude:b"]);
}

#[test]
fn one_tick_charges_each_launch_to_its_account_so_launches_spread_and_stop_at_the_five_hour_stop() {
    let room = |five: f64, week: f64| Room { five_hour: five, week, week_reset_h: 100.0 };
    let accounts = vec![
        sized("claude:a", &["gex45"], 4.0, room(0.6, 50.0)),
        sized("claude:b", &["gex45"], 4.0, room(0.6, 50.0)),
        sized("claude:c", &["gex45"], 1.0, room(1.5, 10.0)),
    ];
    let hosts = vec![host("gex45", 24, Ok(0))];
    assert_eq!(accounts_of(&chosen(hosts.clone(), accounts.clone(), 6)), ["claude:a", "claude:b", "claude:a", "claude:b", "claude:c"]);
    let boxes = (0..6).map(|number| issue("smashcraft", 20 + number, "- [ ] a", &[])).collect();
    let outcome = decide(&survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts), "test");
    let (_, last) = outcome.skipped.last().unwrap();
    assert!(last.contains("gex45 has no account with a free worker slot"), "{last}");
}

#[test]
fn code_only_boxes_fall_back_to_a_host_with_a_free_slot_when_the_cloud_lane_is_unreachable_or_its_sessions_are_out() {
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
    let pins: Vec<Option<&str>> = outcome.launches.iter().map(|launch| launch.pin.as_deref()).collect();
    assert!(pins[0].is_some_and(|pin| pin.contains("claude:a is not signed in")), "{pins:?}");
    assert_eq!(pins[2], None, "a on nexus is the open account with the most headroom");
    let (_, last) = outcome.skipped.last().unwrap();
    assert!(last.contains("gex45 has no free slot") && last.contains("whiterabbit takes nothing: laptop unavailable"), "{last}");

    let mut reachable = survey(boxes.clone(), Cloud::Reachable("cloud credit: unread".into()), hosts.clone(), accounts.clone());
    reachable.cloud_running = super::cloud::SESSIONS - 1;
    let full = Some("north dispatch: cloud lane open (cloud credit: unread) but its 6 sessions are out".to_owned());
    assert_eq!(
        placed(&reachable),
        [
            ("smashcraft#20".into(), "anthropic-cloud".into(), "claude login on nexus".into(), true, None),
            ("smashcraft#21".into(), "gex45".into(), "claude:b".into(), false, full.clone()),
            ("smashcraft#22".into(), "gex45".into(), "claude:b".into(), false, full.clone()),
            ("smashcraft#23".into(), "nexus".into(), "claude:a".into(), false, full),
        ],
        "a cloud launch takes no host slot"
    );

    let closed = survey(boxes, Cloud::Closed("cloud credit: expired".into()), hosts, accounts);
    assert!(placed(&closed).iter().all(|(_, _, _, cloud, local)| !cloud && local.is_none()));
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
    let entry = |item: &str, at: f64| record::Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: "sc-1-d0000".into(), cloud: false, launched: true, detail: String::new(), tier: None, category: None, arm: false, handoff: None };
    let now = 100_000.0;
    let records = vec![entry("smashcraft#1", now - 600.0), entry("smashcraft#2", now - 4.0 * 3600.0), entry("wisp#3", now - 5.0 * 3600.0), entry("wisp#3", now - 3.0 * 3600.0)];
    let holds = record::holds(&records, now, &BTreeMap::new());
    assert!(holds[&IssueRef::new("smashcraft", 1)].starts_with("north dispatch launch sc-1-d0000 on gex45 10 min ago"));
    assert!(!holds.contains_key(&IssueRef::new("smashcraft", 2)), "an old single launch whose session ended frees the box");
    assert!(holds[&IssueRef::new("wisp", 3)].contains("2 tries in 24 h"));
    let open = BTreeSet::from([IssueRef::new("smashcraft", 1)]);
    assert_eq!(record::recent_on(&records, "gex45", &open, now), 1);
    let listed_by_now = vec![entry("smashcraft#1", now - 20.0 * 60.0)];
    assert_eq!(record::recent_on(&listed_by_now, "gex45", &open, now), 0, "a launch old enough to show in the copied fleet list no longer counts on its own");
}

#[test]
fn running_and_idle_sessions_hold_their_task_or_their_named_issue() {
    let listing = serde_json::json!([
        {"name": "sc-256-ending", "provider": "claude", "account": "c", "status": "working", "task_ref": null, "machine": null},
        {"name": "fixer", "provider": "claude", "account": "b", "status": "idle", "task_ref": "tompassarelli/wisp#21", "machine": "gex45"},
        {"name": "sc-9-old", "provider": "claude", "account": "b", "status": "parked", "task_ref": null, "machine": null},
        {"name": "proxy", "provider": "codex", "account": "pooled", "status": "starting", "task_ref": null, "machine": null}
    ]);
    let sessions = super::fleets::running_sessions(&listing);
    let items: Vec<Vec<String>> = sessions.iter().map(|session| session.items.iter().map(ToString::to_string).collect()).collect();
    assert_eq!(items, [vec!["smashcraft#256".to_owned()], vec!["wisp#21".to_owned()], vec![]]);
    let hosted: Vec<_> = sessions.into_iter().map(|session| (session, "nexus".to_owned())).collect();
    let (elsewhere, holds) = super::fleets::tally(&hosted);
    assert_eq!(holds[&IssueRef::new("wisp", 21)], "fleet session fixer on nexus", "an idle session still holds its issue");
    assert_eq!(elsewhere, BTreeMap::from([("gex45".to_owned(), 1)]));
}

#[test]
fn an_open_gate_on_any_host_holds_each_issue_it_names() {
    let listing = serde_json::json!([
        {"id": 28, "session": "sc-342b", "question": "Malfurion's dash attack (#342): which windup?", "closed_ms": null},
        {"id": 56, "session": "proxy", "question": "smashcraft#2 Hive upload is blocked again; see wisp#39 too", "closed_ms": null},
        {"id": 27, "session": "sc-293", "question": "Melee parity ledger (smashcraft #293)", "closed_ms": null},
        {"id": 9, "session": "sc-300", "question": "Answered smashcraft#301?", "closed_ms": 1000}
    ]);
    let rows = super::survey::gate_rows(&listing, "whiterabbit");
    let items: Vec<String> = rows.iter().map(|(item, _)| item.to_string()).collect();
    assert_eq!(items, ["smashcraft#342", "smashcraft#2", "wisp#39", "smashcraft#293"], "a closed gate holds nothing, and the asking session's name counts");
    assert_eq!(rows[1].1, "open north gate 56 on whiterabbit from proxy, for Tom");
}

#[test]
fn a_box_whose_newest_report_waits_on_tom_or_the_laptop_is_not_ready() {
    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let world = World { open: &open, holds: &holds, planned: &planned, at: 0.0 };
    let reported = |report: &str| issues::readiness(&Issue { report: report.into(), ..issue("smashcraft", 2, "- [ ] upload", &[]) }, &world);
    let why = reported("Upload attempt blocked again.\n\nThe fix waits on Tom's decision in laptop gate 56: bridge or unlock. Nothing was uploaded.").unwrap_err();
    assert_eq!(why, "its newest report waits: The fix waits on Tom's decision in laptop gate 56: bridge or unlock");
    assert!(reported("Not done: the relay run needs the laptop on home Wi-Fi.").unwrap_err().contains("needs the laptop"));
    assert!(reported("Agents do not type passwords, so the next step is Tom's: unlock it once.").is_err());
    assert!(reported("Not done: three judges disagree.\nStop: decision").is_err());
    assert!(reported("Blocked by Tom's sign-in on the private desktop.").is_err());
    assert_eq!(reported("The relay run no longer needs the laptop."), Ok(()), "a negated hand-off hands nothing over");
    assert_eq!(reported("Landed abc123; box 2 is next. Tom asked for a tighter bar (2026-10-25)."), Ok(()), "a report that only mentions Tom hands nothing to him");
    assert_eq!(reported(""), Ok(()));
}

#[test]
fn code_only_boxes_go_to_the_open_laptop_when_the_cloud_lane_and_other_hosts_are_full() {
    let (mut hosts, accounts) = fleet();
    hosts[0].running = Ok(24);
    hosts[1].running = Ok(6);
    hosts[2].running = Ok(7);
    let boxes = vec![issue("smashcraft", 50, "- [ ] a", &[]), issue("smashcraft", 51, "- [ ] b", &[])];
    let mut open = survey(boxes, Cloud::Reachable("cloud credit: unread".into()), hosts.clone(), accounts.clone());
    open.cloud_running = super::cloud::SESSIONS;
    let launched = placed(&open);
    assert_eq!(launched.iter().map(|row| (row.0.as_str(), row.1.as_str(), row.2.as_str(), row.3)).collect::<Vec<_>>(), [("smashcraft#50", "whiterabbit", "claude:a", false)]);
    hosts[2].running = Err("laptop busy".into());
    let busy = survey(vec![issue("smashcraft", 50, "- [ ] a", &[])], Cloud::Closed(String::new()), hosts, accounts);
    let outcome = decide(&busy, "test");
    assert!(outcome.launches.is_empty() && outcome.skipped[0].1.contains("whiterabbit takes nothing: laptop busy"), "{:?}", outcome.skipped);
}

#[test]
fn a_live_threads_claim_on_any_hosts_ledger_holds_its_issue_and_a_stale_one_does_not() {
    let listing = serde_json::json!([
        {"item": "wisp#21", "owner": "sc-21-a", "standing": "live"},
        {"item": "smashcraft#9", "owner": "w4", "standing": "live"},
        {"item": "smashcraft#10", "owner": "w5", "standing": "stale"},
    ]);
    let rows = super::survey::claim_rows(&listing, "gex45", &Default::default());
    assert_eq!(rows, [(IssueRef::new("wisp", 21), "threads claim by sc-21-a on gex45".to_owned()), (IssueRef::new("smashcraft", 9), "threads claim by w4 on gex45".to_owned())]);
    assert!(super::survey::claim_rows(&serde_json::json!([]), "nexus", &Default::default()).is_empty());
}

#[test]
fn an_account_placement_never_judged_gets_no_slot() {
    let control = serde_json::json!({"at": 1000.0, "allowed_workers": 2});
    let judged = gate::Verdict { account: "claude:b".into(), available: true, headroom: 40.0, line: "claude:b ok".into(), owners: vec!["gex45".into()] };
    assert_eq!(super::survey::refusal("claude:b", &control, Some(&judged), 1060.0), None);
    let unjudged = super::survey::refusal("codex", &control, None, 1060.0).unwrap();
    assert!(unjudged.contains("codex has no placement verdict"), "{unjudged}");
    assert!(super::survey::refusal("claude:b", &control, Some(&judged), 1000.0 + 16.0 * 60.0).unwrap().contains("sampled 16 min ago"));
}

#[test]
fn the_laptops_copied_lists_hold_their_issues_even_when_old() {
    let directory = tempfile::tempdir().unwrap();
    let mut notes = Vec::new();
    assert!(super::fleets::mirrored(directory.path(), "fleet", "whiterabbit", 0.0, &mut notes).is_null());
    assert!(notes[0].starts_with("whiterabbit fleet list unread"), "{notes:?}");
    let listing = serde_json::json!([{"name": "fixer", "provider": "claude", "account": "a", "status": "working", "task_ref": "tompassarelli/smashcraft#350", "machine": null}]);
    std::fs::write(directory.path().join("fleet-whiterabbit.json"), listing.to_string()).unwrap();
    let gates = serde_json::json!([{"id": 56, "session": "sc-2-hive2", "question": "smashcraft#2 upload", "closed_ms": null}]);
    std::fs::write(directory.path().join("gates-whiterabbit.json"), gates.to_string()).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs_f64();
    let mut notes = Vec::new();
    let old = super::fleets::running_sessions(&super::fleets::mirrored(directory.path(), "fleet", "whiterabbit", now + 3600.0, &mut notes));
    assert_eq!(old.iter().map(|session| session.items.clone()).collect::<Vec<_>>(), [vec![IssueRef::new("smashcraft", 350)]]);
    let hosted: Vec<_> = old.into_iter().map(|session| (session, "whiterabbit".to_owned())).collect();
    assert_eq!((super::survey::laptop_running(&hosted, "whiterabbit", 0), super::survey::laptop_running(&hosted, "whiterabbit", 3)), (1, 3), "the copied list counts the laptop's sessions, and a fresh launch not yet listed still counts");
    assert!(notes[0].starts_with("whiterabbit fleet list copied 60 min ago"), "{notes:?}");
    let gated = super::survey::gate_rows(&super::fleets::mirrored(directory.path(), "gates", "whiterabbit", now, &mut notes), "whiterabbit");
    assert_eq!(gated[0].0, IssueRef::new("smashcraft", 2));
}

#[test]
fn an_unticked_native_done_when_box_or_native_evidence_label_keeps_a_box_out_of_the_cloud_lane() {
    let (hosts, accounts) = fleet();
    let tagged = issue("smashcraft", 50, "- [x] (headless run) checks pass\n- [ ] (native) three cold viewers call it one chop", &[]);
    let labelled = issue("smashcraft", 51, "- [ ] a", &["evidence:native"]);
    let landed = issue("smashcraft", 52, "- [x] (native) captured\n- [ ] (headless run) checks pass", &[]);
    let reachable = survey(vec![tagged, labelled, landed], Cloud::Reachable("cloud credit: unread".into()), hosts, accounts);
    let launched = placed(&reachable);
    let couriers: Vec<&str> = launched.iter().filter(|row| row.3).map(|row| row.0.as_str()).collect();
    assert_eq!(couriers, ["smashcraft#52"], "only the box whose native box is ticked is code-only: {launched:?}");
    assert!(launched.iter().filter(|row| !row.3).all(|row| row.1 == "gex45"), "native smashcraft boxes go to the host serving native: {launched:?}");
}

#[test]
fn a_box_launched_on_the_laptop_or_here_is_held_to_that_host_and_a_remote_one_names_its_host_in_the_cwd() {
    let (hosts, accounts) = fleet();
    let survey = survey(Vec::new(), Cloud::Closed(String::new()), hosts, accounts);
    let launch = |host: &str, local: Option<&str>| super::plan::Launch {
        item: issue("smashcraft", 7, "- [ ] a", &[]).reference(),
        host: host.into(),
        account: "claude:c".into(),
        brief: "brief".into(),
        route: super::tier_tests::route(super::tier::Tier::Opus),
        cloud: false,
        local: local.map(str::to_owned),
        pin: None,
        handoff: None,
    };
    let local = |host: &str, given: Option<&str>| {
        let (_, words) = super::effects::command(&launch(host, given), "sc-7", &survey, "/home/tom", "north").unwrap();
        let at = words.iter().position(|word| word == "--local");
        (words.iter().any(|word| word.starts_with(&format!("{host}:"))), at.map(|at| words[at + 1].clone()))
    };
    assert_eq!(local("whiterabbit", None), (false, Some("north dispatch placed this box on whiterabbit".into())));
    assert_eq!(local("nexus", None), (false, Some("north dispatch placed this box on nexus".into())));
    assert_eq!(local("nexus", Some("cloud lane unreachable")), (false, Some("cloud lane unreachable".into())));
    assert_eq!(local("gex45", None), (true, None));
}

#[test]
fn an_autoland_cloud_launch_runs_from_its_repo_checkout_with_trust_written_and_a_brief_ending_in_a_land_push() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".claude.json");
    std::fs::write(&config, r#"{"oauthAccount":{"organizationUuid":"org"},"projects":{"/elsewhere":{"hasTrustDialogAccepted":true}}}"#).unwrap();
    let box_brief = super::brief::brief(&issue("smashcraft", 60, "- [ ] a\nSurface: rust", &[]), "test", &super::tier_tests::route(super::tier::Tier::Opus));
    let text = super::brief::cloud(&box_brief, "smashcraft", &super::cloud::branch("sc-60-d1200"));
    let mut calls: Vec<(String, String, Vec<String>)> = Vec::new();
    let mut trusted_at_launch = None;
    let mut run = |cwd: &std::path::Path, program: &str, args: &[String]| -> Result<String, String> {
        calls.push((cwd.display().to_string(), program.to_owned(), args.to_vec()));
        if program == "claude" {
            let written: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
            trusted_at_launch = Some(written["projects"]["/tmp/cloud/src/smashcraft"]["hasTrustDialogAccepted"].clone());
            return Ok("Created cloud session: https://claude.ai/code/session_01AbcXyz\n".into());
        }
        Ok(String::new())
    };
    let launched = super::cloud::launch("smashcraft", "sc-60-d1200", &text, &[], std::path::Path::new(super::cloud::SOURCES), home.path(), &mut run).unwrap();

    assert_eq!(launched, super::cloud::Launched { session: "session_01AbcXyz".into(), branch: "claude/land-sc-60-d1200".into(), directory: "/tmp/cloud/src/smashcraft".into() });
    assert_eq!(trusted_at_launch, Some(serde_json::Value::Bool(true)), "trust is written before claude --cloud starts");
    let (cwd, program, args) = calls.last().unwrap();
    assert_eq!((cwd.as_str(), program.as_str(), args[0].as_str(), args[1].as_str()), ("/tmp/cloud/src/smashcraft", "claude", "--cloud", text.as_str()));
    let git: Vec<String> = calls.iter().filter(|(_, program, _)| program == "git").map(|(_, _, args)| args.join(" ")).collect();
    let dir = "-C /tmp/cloud/src/smashcraft";
    assert_eq!(
        git[git.len() - 4..],
        [
            format!("{dir} fetch -q --depth 1 --filter=blob:none origin main"),
            format!("{dir} sparse-checkout set --no-cone /README.md"),
            format!("{dir} checkout -q -B main FETCH_HEAD"),
            format!("{dir} branch -q --set-upstream-to=origin/main main"),
        ]
    );
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
    assert_eq!((kept["oauthAccount"]["organizationUuid"].as_str(), kept["projects"]["/elsewhere"]["hasTrustDialogAccepted"].as_bool()), (Some("org"), Some(true)));

    let last = text.rsplit(". Last, ").next().unwrap();
    assert!(last.starts_with("push with `git push origin HEAD:refs/heads/claude/land-sc-60-d1200`") && last.contains("Done: queued claude/land-sc-60-d1200") && last.contains("no courier runs"), "{last}");
    assert!(!last.contains("BEGIN-HANDBACK"), "the handback is only the refused-push fallback");
    for needed in [super::brief::SAFETY_LINE, "git config user.name tompassarelli", "git config user.email 6378995+tompassarelli@users.noreply.github.com", "Claude-Session:", "Co-Authored-By:", "git reset --soft origin/main", "Closes #<number>"] {
        assert!(text.contains(needed), "{needed}");
    }
}

#[test]
fn a_fresh_launch_checkout_starts_with_init_and_every_refresh_sets_the_public_origin() {
    let steps = super::cloud::checkout_steps("wisp", std::path::Path::new("/tmp/cloud/src/wisp"), true);
    assert_eq!(steps[0].join(" "), "init -q /tmp/cloud/src/wisp");
    assert_eq!(steps[1].join(" "), "-C /tmp/cloud/src/wisp config remote.origin.url https://github.com/tompassarelli/wisp.git");
    let refreshed = super::cloud::checkout_steps("wisp", std::path::Path::new("/tmp/cloud/src/wisp"), false);
    assert_eq!(refreshed[..], steps[1..], "a refresh sets the origin too, so a half-made checkout heals");
}

#[test]
fn a_north_cloud_brief_pushes_its_land_branch_for_a_courier_and_does_not_close_the_issue() {
    let text = super::brief::cloud("Item: north#9 x", "north", "claude/land-n9");
    let last = text.rsplit(". Last, ").next().unwrap();
    assert!(last.contains("Done: pushed claude/land-n9") && last.contains("a courier off the cloud host lands it through safe-push"), "{last}");
    assert!(!text.contains("Closes #"));
}

#[test]
fn a_cloud_launch_holds_its_box_six_hours_and_counts_against_the_lane() {
    let now = 100_000.0;
    let cloud = |item: &str, at: f64| record::Record { at, item: item.into(), host: "anthropic-cloud".into(), account: "claude login on nexus".into(), name: "sc-7-d0000".into(), cloud: true, launched: true, detail: String::new(), tier: None, category: None, arm: false, handoff: None };
    let records = vec![cloud("smashcraft#7", now - 5.0 * 3600.0), cloud("smashcraft#8", now - 7.0 * 3600.0)];
    let holds = record::holds(&records, now, &BTreeMap::new());
    assert!(holds.contains_key(&IssueRef::new("smashcraft", 7)) && !holds.contains_key(&IssueRef::new("smashcraft", 8)));
    let open = BTreeSet::from([IssueRef::new("smashcraft", 7), IssueRef::new("smashcraft", 8)]);
    assert_eq!(record::recent_on(&records, "anthropic-cloud", &open, now), 1);
}

#[test]
fn a_cloud_launch_gives_claude_a_terminal_and_reads_the_session_through_its_control_sequences() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".claude.json"), "{}").unwrap();
    std::fs::create_dir(home.path().join("wisp")).unwrap();
    let claude = home.path().join("claude");
    std::fs::write(&claude, "#!/bin/sh\n[ -t 0 ] && [ -t 1 ] && [ -t 2 ] || exit 1\nprintf '\\033[1mCreated\\033[0m session_01Test\\n'\n").unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut run = |cwd: &std::path::Path, program: &str, args: &[String]| if program == "git" { Ok(String::new()) } else { super::cloud::run_from(cwd, program, &claude, args) };
    let launched = super::cloud::launch("wisp", "wisp-9-d1200", "brief", &[], home.path(), home.path(), &mut run);

    assert_eq!(launched.map(|launched| launched.session), Ok("session_01Test".to_owned()));
}

#[test]
fn a_launch_that_never_started_is_no_try_and_holds_the_box_only_briefly() {
    let failed = |item: &str, at: f64| record::Record { at, item: item.into(), host: "anthropic-cloud".into(), account: "claude:a".into(), name: "sc-9-d0000".into(), cloud: true, launched: false, detail: "claude --cloud: exit status: 1".into(), tier: None, category: None, arm: false, handoff: None };
    let now = 100_000.0;
    let records = vec![failed("smashcraft#9", now - 3600.0), failed("smashcraft#9", now - 1800.0), failed("wisp#4", now - 300.0)];
    let holds = record::holds(&records, now, &BTreeMap::new());
    assert!(!holds.contains_key(&IssueRef::new("smashcraft", 9)), "two failed launches are no tries and their brief hold has passed: {holds:?}");
    assert!(holds[&IssueRef::new("wisp", 4)].starts_with("north dispatch launch sc-9-d0000"), "a fresh failure still holds the box for a few minutes");
}
