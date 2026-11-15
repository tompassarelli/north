use super::issues::Issue;
use super::slots::{Account, Host, PlanSize, Room};
use super::survey::Survey;
use super::decide;
use crate::spend::{self, Rules, View, Window};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const CONFIG: &str = r#"
[hosts.nexus]
resident_cap = 6
[hosts.gex45]
resident_cap = 24
ssh = "tom@gex45"
serves = ["native:makgora", "warcraft:makgora"]
[hosts.whiterabbit]
resident_cap = 8
serves = ["native", "warcraft", "sign-ins", "laptop"]
[placement]
order = ["gex45", "anthropic-cloud", "nexus", "laptop"]
private_repos = ["galileo"]
[projects.galileo]
priority = 2
repos = ["galileo"]
[projects.makgora]
priority = 3
repos = ["makgora", "south"]
dispatch_repos = ["makgora"]
[projects.nexus]
priority = 5
repos = ["firn"]
dispatch_repos = []
[projects.wisp]
priority = 4
repos = ["wisp"]
[projects.north]
priority = 1
repos = ["north"]
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

pub(super) fn host(name: &str, cap: i64, running: Result<i64, &str>) -> Host {
    Host { name: name.into(), cap: Some(cap), room: Some(cap), running: running.map_err(str::to_owned), measure: None, builds: None }
}

fn account(name: &str, hosts: &[&str], headroom: f64) -> Account {
    sized(name, hosts, MAX_5X, Room { five_hour: headroom, week: headroom, week_reset_h: 100.0 })
}

pub(super) const MAX_5X: PlanSize = PlanSize { five_hour: 1.0, week: 1.0 };

pub(super) fn sized(name: &str, hosts: &[&str], plan: PlanSize, room: Room) -> Account {
    Account { name: name.into(), hosts: hosts.iter().map(|host| (*host).to_owned()).collect(), plan, room, grant: None }
}

/// The spend view of `accounts`: each one's readings are its room, so an account with no five-hour
/// room is at its provider's limit.
pub(super) fn view(accounts: &[Account]) -> View {
    let window = |left: f64, reset_h: f64| Some(Window { used_pct: 100.0 - left, resets_at: Some(reset_h * 3600.0) });
    let spent = |account: &Account| spend::Account { name: account.name.clone(), plan_week: account.plan.week, five_hour: window(account.room.five_hour, 5.0), week: window(account.room.week, account.room.week_reset_h), counted: true };
    View { at: 0.0, accounts: accounts.iter().map(spent).collect(), working: 0, operational: Vec::new(), held: None }
}

/// The shipped rules, so these tests pin slots and order alone.
pub(super) fn rules() -> Rules {
    Rules::configured()
}

pub(super) fn issue(repo: &str, number: u64, body: &str, labels: &[&str]) -> Issue {
    Issue { repo: repo.into(), number, title: format!("box {number}"), body: format!("{body}\nCategory: feature"), labels: labels.iter().map(|label| (*label).to_owned()).collect(), ..Issue::default() }
}

pub(super) fn survey(issues: Vec<Issue>, hosts: Vec<Host>, accounts: Vec<Account>) -> Survey {
    let open = issues.iter().map(Issue::reference).collect();
    Survey {
        here: "nexus".into(),
        home: Some("/home/tom".into()),
        cfg: CONFIG.parse().unwrap(),
        projects: crate::threads::projects::Projects::read(&CONFIG.parse().unwrap()).unwrap(),
        hosts,
        spend: view(&accounts),
        rules: rules(),
        accounts,
        issues,
        open,
        holds: BTreeMap::new(),
        box_holds: BTreeMap::new(),
        predicted: BTreeMap::new(),
        box_tries: BTreeMap::new(),
        mainline: super::retired::Mainline::default(),
        live: BTreeSet::new(),
        listed: BTreeSet::new(),
        sessions: Vec::new(),
        laptop: Some("whiterabbit".into()),
        notes: Vec::new(),
        frozen: Vec::new(),
        history: super::tier::History::default(),
        ledger: BTreeMap::new(),
        grounds: super::grounds::Ledger::default(),
        strands: Vec::new(),
        saturated: BTreeMap::new(),
        line_held: None,
        revived: BTreeMap::new(),
        backlog: None,
        deepseek: super::deepseek::Seen::default(),
        learned: Vec::new(),
        at: 0.0,
    }
}

pub(super) fn fleet() -> (Vec<Host>, Vec<Account>) {
    let hosts = vec![host("gex45", 24, Ok(22)), host("nexus", 6, Ok(5)), host("whiterabbit", 8, Err("laptop unavailable"))];
    let out = sized("claude:c", &["gex45"], MAX_5X, Room { five_hour: 0.0, week: 90.0, week_reset_h: 100.0 });
    let accounts = vec![account("claude:a", &["nexus", "whiterabbit"], 70.0), account("claude:b", &["gex45"], 40.0), out];
    (hosts, accounts)
}

#[test]
fn a_claude_routed_box_launches_under_the_cap_and_never_on_the_cloud_lane() {
    super::spread_tests::a_host_whose_admission_queue_is_at_its_limit_or_whose_root_disk_is_past_the_reapers_eviction_line_takes_no_session();
    super::spread_tests::a_host_whose_pushed_github_token_is_past_its_trust_takes_no_session_and_its_box_places_on_another_host();
    super::spread_tests::launches_earlier_in_the_run_count_against_each_hosts_sessions_and_admission_queue();
    super::spread_tests::every_build_hosts_line_over_the_wait_bar_holds_each_box_with_its_expected_wait();
    let (mut hosts, accounts) = fleet();
    let queued = "gex45 skipped: 14.0 GiB free on the root disk, under 15";
    hosts[0] = hosts[0].clone().saturated(Some(queued.into()));
    let boxes = vec![issue("makgora", 50, "- [ ] a", &[]), issue("makgora", 51, "- [ ] b", &["needs:native-pair"])];
    let outcome = decide(&survey(boxes, hosts, accounts), "test");
    let launched: Vec<(String, &str)> = outcome.launches.iter().map(|launch| (launch.item.to_string(), launch.host.as_str())).collect();
    assert_eq!(launched, [("makgora#50".to_owned(), "nexus")]);
    let (item, why) = outcome.skipped.last().unwrap();
    assert_eq!(item.to_string(), "makgora#51");
    assert!(why.contains(&format!("gex45 takes nothing: {queued}")), "{why}");

    let (hosts, accounts) = fleet();
    let boxes = vec![issue("makgora", 60, "- [ ] a", &[]), issue("makgora", 61, "- [ ] b", &[])];
    let mut open = survey(boxes.clone(), hosts, accounts);
    open.rules.claude_sessions = 1;
    let capped = decide(&open, "test");
    assert_eq!(capped.launches.len(), 1, "{:?}", capped.skipped);
    assert_ne!(capped.launches[0].host, super::cloud::HOST, "a claude box never goes to the cloud lane");
    assert!(capped.skipped.iter().any(|(_, why)| why.contains("dispatch_claude_sessions cap of 1")), "{:?}", capped.skipped);

    let (hosts, _) = fleet();
    let full = vec![account("claude:a", &["nexus"], 5.0)];
    let refused = decide(&survey(boxes, hosts, full), "test");
    assert!(refused.launches.is_empty(), "{:?}", refused.launches);
    assert!(refused.skipped.iter().any(|(_, why)| why.contains("money ceiling")), "{:?}", refused.skipped);
}

#[test]
fn an_issue_whose_every_open_box_names_a_live_owner_or_whose_planning_box_adds_an_agent_with_no_simplest_line_is_skipped_and_others_are_staffed() {
    let open = BTreeSet::new();
    let (holds, planned) = (BTreeMap::new(), BTreeMap::new());
    let live: BTreeSet<String> = ["n-land-first", "n-cull-it", "n-cull-landing", "lead-88"].map(str::to_owned).into();
    let world = super::issues::World { open: &open, holds: &holds, box_holds: &BTreeMap::new(), box_tries: &BTreeMap::new(), live: &live, planned: &planned, backlog: None, at: 0.0 };
    let ready = |body: &str| super::issues::readiness(&issue("north", 633, body, &[]), &world);
    assert_eq!(ready("- [x] done (owner: n-fast-land)\n- [ ] cores first (owner: n-land-first)"), Err("every open box has a live owner: n-land-first".into()));
    assert_eq!(ready("- [ ] cull north (owners: n-cull-it, -landing)\n  - [ ] code: delete it. Owner: lead-88."), Err("every open box has a live owner: lead-88, n-cull-it, n-cull-landing".into()));
    assert_eq!(ready("- [ ] cores first (owner: n-land-first)\n- [ ] makgora culled the same way"), Ok(()), "a box with no owner is open to staff");
    assert_eq!(ready("- [ ] spans by category (owner: n-land-profile)"), Ok(()), "an owner the fleet does not list as live holds nothing");
    let refused = |adds: &str| Err(format!("a planning or design box adds {adds} with no `Simplest:` line naming the smallest change that meets its Done when and why it falls short"));
    assert_eq!(ready("Category: planning\n- [ ] Plan a new review agent for landings"), refused("agent"));
    assert_eq!(ready("Category: feature\n- [ ] Pick between design options: spin up a cache service"), refused("service"));
    assert_eq!(ready("Category: planning\n- [ ] Run a bake-off of the two routers"), refused("bake-off"));
    assert_eq!(ready("Category: planning\nSimplest: one more column in the ledger; it cannot rank tiers\n- [ ] Plan a new review agent"), Ok(()));
    assert_eq!(ready("Category: feature\n- [ ] Add a new crate for the probe"), Ok(()), "a feature box with no design work is not held");
    assert_eq!(ready("Category: planning\n- [x] a new agent\n- [ ] tick the plan"), Ok(()), "only open boxes count");
}

#[test]
fn a_dispatcher_with_no_api_key_places_on_the_host_that_holds_one() {
    let (hosts, _) = fleet();
    let low = Room { five_hour: 90.0, week: 5.0, week_reset_h: 100.0 };
    let plans = vec![sized("claude:a", &["nexus"], MAX_5X, low), sized("claude:b", &["gex45"], MAX_5X, low)];
    let policy: toml_edit::DocumentMut = "[accounts.claude]\napi_beside_plans = true\n\n[accounts.claude.api_orgs.zenith]\nid = \"org\"\nkeys = { nexus = [\"/run/secrets/key\"], gex45 = [\"/run/secrets/key\"] }\n".parse().unwrap();
    let source = north_strategy_types::placement::GrantSource { account: "claude:api:zenith".into(), left_usd: 500.0, deadline_h: 200.0, urgent: true, line: String::new() };
    let grants = super::survey::grants(&policy, &crate::fleet::roster::Roster::default(), "nexus", vec![source]);
    assert_eq!(grants.iter().map(|grant| (grant.name.as_str(), grant.hosts.clone())).collect::<Vec<_>>(), [("claude:api:zenith", vec!["gex45".to_owned()])], "nexus dispatch dropped every grant because its own roster holds no api key (north#723)");

    let mut surveyed = survey(vec![issue("makgora", 7, "- [ ] a", &[])], hosts, plans.iter().cloned().chain(grants).collect());
    surveyed.spend = view(&plans);
    let outcome = decide(&surveyed, "test");
    assert_eq!(outcome.launches.iter().map(|launch| (launch.host.as_str(), launch.account.as_str())).collect::<Vec<_>>(), [("gex45", "claude:api:zenith")], "{:?}", outcome.skipped);
}

/// north#835: on 2026-11-08 merging issues left 14 open boxes on north#719 and 28 on north#706, run one at a time; north-740-d1801 spent a first try on a box whose window over readings had not passed, which its `(not before ...)` stamp now holds.
#[test]
fn independent_boxes_launch_side_by_side_while_an_overlapping_a_tried_a_windowed_and_a_predicted_write_hotspots_box_wait() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let first = "the claims replay in `crates/north-fleet/src/fleet/claims.rs` passes";
    let body = format!("- [ ] {first}\n- [ ] holds in crates/north-dispatch/src/dispatch/hotspots.rs\n- [ ] the tip merges into origin/main\n- [ ] the tag sits on origin/main\n- [ ] harness/skills/workers/SKILL.md says so\n- [ ] the ready list in `crates/north-dispatch/src/dispatch` names each box\n- [ ] Over 3 h no launch is refused (not before 2099-01-01T00:00Z)");
    let surveyed = |tried: &[&str]| {
        let mut survey = survey(vec![issue("north", 835, &body, &[])], hosts.clone(), accounts.clone());
        survey.box_tries.insert(super::issues::IssueRef::new("north", 835), tried.iter().filter_map(|text| Some((crate::threads::claims::box_slug(text)?, "tried twice".to_owned()))).collect());
        decide(&survey, "test")
    };
    let boxes = |outcome: &super::Outcome| outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect::<Vec<_>>();
    let outcome = surveyed(&[]);
    assert_eq!(boxes(&outcome), [first, "holds in crates/north-dispatch/src/dispatch/hotspots.rs", "the tip merges into origin/main", "the tag sits on origin/main", "harness/skills/workers/SKILL.md says so"], "the box stamped with a later date waits for it, and boxes that both name the ref origin/main launch side by side");
    let held: Vec<&String> = outcome.skipped.iter().map(|(_, why)| why).collect();
    assert!(held.len() == 1 && held[0].contains("box `the ready list") && held[0].contains("both name crates/north-dispatch/src/dispatch"), "{held:?}");
    let tried = surveyed(&[first]);
    assert_eq!(boxes(&tried).len(), 4, "a box tried twice waits for the coordinator alone, and its issue's other boxes still launch: {:?}", tried.skipped);
    let predicting = |body: &str| {
        let mut survey = survey(vec![issue("north", 835, body, &[])], hosts.clone(), accounts.clone());
        survey.predicted.insert("fleet session north-900-x on nexus".into(), vec!["crates/north-fleet/src/fleet/claims.rs".into()]);
        decide(&survey, "test")
    };
    let predicted = predicting(&body);
    assert_eq!(boxes(&predicted).len(), 4, "a box naming a running session's predicted write waits while the others launch: {:?}", predicted.skipped);
    assert!(predicted.skipped.iter().any(|(_, why)| why.contains("held for fleet session north-900-x on nexus: predicted write set names crates/north-fleet/src/fleet/claims.rs")), "{:?}", predicted.skipped);
    let alone = predicting(&format!("- [ ] {first}"));
    assert_eq!(boxes(&alone), [first], "a box held only by a predicted write launches when nothing else is ready: {:?}", alone.skipped);
}

fn over_bound_parent(number: u64, leaves: u64) -> Issue {
    let children = super::issues::Children { total: leaves, ..super::issues::Children::default() };
    Issue { children, ..issue("north", number, "- [ ] every sub-issue is closed", &["roadmap"]) }
}

fn leaf_under(of: u64, number: u64) -> Issue {
    Issue { parent: Some(super::issues::ParentRef::new(super::issues::OWNER, "north", of)), ..issue("north", number, "Spec: exact", &[]) }
}

fn lead_bounded(issues: Vec<Issue>) -> Survey {
    let (_, accounts) = fleet();
    let mut world = survey(issues, vec![host("gex45", 24, Ok(0)), host("nexus", 6, Ok(0))], accounts);
    world.cfg = format!("{CONFIG}\n[dispatch]\nlead_reports = 8\nleads = 1\n").parse().unwrap();
    world
}

#[test]
fn a_parent_over_the_bound_gets_one_lead_and_loses_it_when_its_subtree_closes_and_a_hand_launched_lead_is_never_stopped_by_dispatch() {
    let mut issues = vec![over_bound_parent(1000, 9), over_bound_parent(2000, 10)];
    issues.extend((1..=9).map(|n| leaf_under(1000, 1000 + n)));
    issues.extend((1..=10).map(|n| leaf_under(2000, 2000 + n)));
    let mut quiet = lead_bounded(issues.clone());
    let bounded = std::mem::replace(&mut quiet.cfg, CONFIG.parse().unwrap());
    let unset = decide(&quiet, "north fleet dispatch on nexus");
    assert!(unset.tree.over.is_empty() && !unset.launches.iter().any(|launch| launch.lineage.lead), "with no lead bound in the orchestration config dispatch staffs no lead");
    quiet.cfg = bounded;

    let first = decide(&quiet, "north fleet dispatch on nexus");
    let over: Vec<String> = first.tree.over.iter().map(ToString::to_string).collect();
    assert_eq!(over, ["lead north#2000: load 10 over 8", "lead north#1000: load 9 over 8"], "a dry run names each parent over the bound, the largest first");
    let leads: Vec<_> = first.launches.iter().filter(|launch| launch.lineage.lead).collect();
    assert_eq!(leads.len(), 1, "[dispatch] leads admits one lead at a time: {leads:?}");
    assert_eq!((leads[0].item.to_string(), super::effects::session_name(leads[0], 0.0), leads[0].lineage.parent.clone()), ("north#2000".to_owned(), "lead-north-2000".to_owned(), None));
    assert!(leads[0].brief.contains("Category: review") && leads[0].brief.contains("references/lead.md"), "{}", leads[0].brief);
    let leaf = first.launches.iter().find(|launch| launch.item.number == 1001).expect("an open leaf under a parent launches whole");
    assert!(leaf.brief.lines().any(|line| line.starts_with("Item: north#1001 ")) && !leaf.brief.lines().any(|line| line.starts_with("Box: ")), "{}", leaf.brief);
    assert!(leaf.brief.lines().any(|line| line.starts_with("Done when: north#1001 is closed by its check")), "{}", leaf.brief);

    let hand = hand_lead("burn-lead");
    let mut led = lead_bounded(issues.clone());
    led.sessions = vec![(hand.clone(), "nexus".into())];
    led.live.insert(hand.name);
    led.holds.insert(super::IssueRef::new("north", 2000), "fleet session burn-lead on nexus".into());
    let second = decide(&led, "north fleet dispatch on nexus");
    let staffed: Vec<String> = second.launches.iter().filter(|launch| launch.lineage.lead).map(|launch| launch.item.to_string()).collect();
    assert_eq!(staffed, ["north#1000"], "a hand-launched lead under another name leads north#2000, and fills no dispatched lead's room");
    let under: Vec<_> = second.launches.iter().filter(|launch| launch.item.number > 2000).collect();
    assert!(!under.is_empty() && under.iter().all(|launch| launch.lineage.parent.as_deref() == Some("burn-lead")), "{under:?}");
    assert!(second.tree.stops.is_empty());

    led.cfg = CONFIG.parse().unwrap();
    assert!(decide(&led, "north fleet dispatch on nexus").launches.iter().all(|launch| launch.lineage == super::plan::Lineage::default()), "with no lead bound dispatch staffs no lead and parents no launch under one");

    let mut closed = lead_bounded(issues[..11].to_vec());
    closed.sessions = vec![(hand_lead("lead-north-2000"), "nexus".into())];
    let kept = decide(&closed, "north fleet dispatch on nexus");
    let staffed: Vec<String> = kept.launches.iter().filter(|launch| launch.lineage.lead).map(|launch| launch.item.to_string()).collect();
    assert!(kept.tree.stops.is_empty(), "a lead dispatch did not launch stays whatever its name: {:?}", kept.tree.stops);
    assert_eq!(staffed, ["north#1000"], "a hand-launched lead under the dispatched name counts toward no dispatched lead's room");
    let lead = super::fleets::Session { dispatched: true, ..hand_lead("lead-north-2000") };
    closed.sessions = vec![(lead.clone(), "nexus".into())];
    let stops: Vec<String> = decide(&closed, "north fleet dispatch on nexus").tree.stops.iter().map(|stop| stop.why.clone()).collect();
    assert_eq!(stops, ["nothing under north#2000 is ready and no child of it lives"]);
    closed.sessions.push((super::fleets::Session { name: "north-2003-d0100".into(), parent: Some(lead.name.clone()), lead: false, dispatched: false, ..lead.clone() }, "nexus".into()));
    assert!(decide(&closed, "north fleet dispatch on nexus").tree.stops.is_empty(), "a lead with a live child stays");
    closed.sessions = vec![(lead, "nexus".into())];
    closed.cfg = CONFIG.parse().unwrap();
    assert!(decide(&closed, "north fleet dispatch on nexus").tree.stops.is_empty(), "with no lead bound in the orchestration config dispatch stops no lead");
}

fn hand_lead(name: &str) -> super::fleets::Session {
    super::fleets::Session { name: name.into(), items: vec![super::IssueRef::new("north", 2000)], claim_box: None, machine: None, working: true, account: Some("claude:a".into()), worker: true, predicted: Vec::new(), parent: None, lead: true, dispatched: false }
}

/// north#1345: dispatch on nexus staffed C3, C4 and L10 of north#1276 on 2026-11-13 while its body listed them under `## Parked (gate 242, 2026-11-13)`.
#[test]
fn a_box_under_the_parked_heading_gets_no_launch_while_the_boxes_outside_it_still_launch() {
    let parked = "## Parked (gate 242, 2026-11-13)\nParked by the owner's decision on gate 242 (default at 02:20 UTC): not staffed until moved back.\n\n- [ ] Argument parsing and error handling go through a library chosen by a recorded measurement.\n  - [ ] C1 Convert north (`src/cli`, 88 flag arms, 245 parse-function lines) to clap derive with help generation off.\n  - [ ] C2 Convert north-ops (75 flag arms, 250 parse-function lines).\n  - [ ] C3 Convert north-core (59 flag arms).\n  - [ ] C4 Convert north-lander, north-runtime, north-flow and north-lanes, one landing each.\n  - [ ] L10 REWRITE: the guard hook dispatches from one table in `crates/north-guard/src/hook.rs`.\n";
    let body = format!("## Done when\n- [ ] L03 Check: the code-health report lists each crate's layer\n\n{parked}\n## Filed later\n- [ ] L05 Move: the lander's notices leave north-core\n");
    let (hosts, accounts) = fleet();
    let outcome = decide(&survey(vec![issue("north", 1276, &body, &[])], hosts, accounts), "test");
    let mut staffed: Vec<&str> = outcome.launches.iter().flat_map(|launch| launch.brief.lines().filter_map(|line| line.strip_prefix("Box: "))).collect();
    staffed.sort_unstable();
    assert_eq!(staffed, ["L03 Check: the code-health report lists each crate's layer", "L05 Move: the lander's notices leave north-core"], "{:?}", outcome.skipped);
    let (hosts, accounts) = fleet();
    let only_parked = decide(&survey(vec![issue("north", 1276, parked, &[])], hosts, accounts), "test");
    assert!(only_parked.launches.is_empty(), "{:?}", only_parked.launches.iter().map(|launch| &launch.brief).collect::<Vec<_>>());
    let skipped: Vec<(String, &str)> = only_parked.skipped.iter().map(|(item, why)| (item.to_string(), why.as_str())).collect();
    assert_eq!(skipped, [("north#1276".to_owned(), "every open box waits, the first until it is moved out from under the issue's `## Parked` heading")]);
}

#[test]
fn a_launch_reads_the_listing_the_keeper_recorded_without_asking_the_host() {
    let records = tempfile::tempdir().unwrap();
    let listing = serde_json::json!([{"name": "north-1054-a"}]);
    super::fleets::record_shared(&records.path().join("fleet-cpx62.json"), &listing);
    let read = super::fleets::shared_read(records.path(), "fleet-cpx62", || Err("the host was asked".to_owned()));
    assert_eq!(read, Ok(listing));
}
