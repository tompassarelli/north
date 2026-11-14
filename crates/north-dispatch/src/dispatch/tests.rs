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
    Host { name: name.into(), cap: Some(cap), room: None, running: running.map_err(str::to_owned), measure: None, builds: None }
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
fn a_cloud_launch_gives_claude_a_terminal_and_reads_the_session_through_its_control_sequences() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".claude.json"), "{}").unwrap();
    std::fs::create_dir(home.path().join("wisp")).unwrap();
    let claude = home.path().join("claude");
    crate::foundation::shim::write_script(&claude, "[ -t 0 ] && [ -t 1 ] && [ -t 2 ] || exit 1\nprintf '\\033[1mCreated\\033[0m session_01Test\\n'\n");
    let mut run = |cwd: &std::path::Path, program: &str, args: &[String]| if program == "git" { Ok(String::new()) } else { super::cloud::run_from(cwd, program, &claude, args) };
    let launched = super::cloud::launch("wisp", "wisp-9-d1200", "brief", &[], home.path(), home.path(), &mut run);

    assert_eq!(launched.map(|launched| launched.session), Ok("session_01Test".to_owned()));

    crate::foundation::shim::write_script(&claude, "printf '\\033[38;2;255;107;128mError:\\033[8GCloud\\033[14Gsessions\\033[23Gneed\\033[28Ga\\033[30Gclaude.ai\\033[40Gsign-in.\\033[39m\\r\\r\\n\\033[?25h\\033(B\\017\\033[?1006l\\0337\\033[r\\0338\\033]9;4;0;\\007\\033(B\\0337\\033[r\\0338'; exit 1\n");
    let refused = super::cloud::launch("wisp", "wisp-9-d1200", "brief", &[], home.path(), home.path(), &mut run);
    assert_eq!(refused.map(|launched| launched.session), Err("claude --cloud: exit status: 1: Error: Cloud sessions need a claude.ai sign-in.".to_owned()), "north cloud launch printed only claude's (B78(B78 reset bytes on 2026-11-06");
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
fn a_project_with_paused_true_gets_no_launch_and_each_of_its_issues_prints_as_paused_while_other_projects_still_launch() {
    let config = CONFIG.replace("[projects.makgora]\n", "[projects.makgora]\npaused = true\n").replace("[projects.wisp]\n", "[projects.wisp]\npaused = true\n").replace("[projects.galileo]\n", "[projects.galileo]\npaused = true\n");
    let cfg: toml_edit::DocumentMut = config.parse().unwrap();
    let (hosts, accounts) = fleet();
    let boxes = vec![issue("makgora", 798, "- [ ] a", &[]), issue("wisp", 230, "- [ ] a", &[]), issue("north", 1, "- [ ] a", &[])];
    let mut paused = survey(boxes, hosts, accounts);
    paused.projects = crate::threads::projects::Projects::read(&cfg).unwrap();
    assert_eq!(paused.projects.set_aside(), ["galileo", "makgora", "wisp"]);
    let outcome = decide(&paused, "test");
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["north#1"]);
    let skipped: Vec<(String, &str)> = outcome.skipped.iter().map(|(item, why)| (item.to_string(), why.as_str())).collect();
    assert_eq!(skipped, [("makgora#798".to_owned(), "project makgora paused paused = true"), ("wisp#230".to_owned(), "project wisp paused paused = true")]);
}

#[test]
fn the_main_red_workflows_boxless_issue_is_not_named_for_a_box() {
    let (hosts, accounts) = fleet();
    let main_red = ["bug", "priority:now", "evidence:code", crate::platform::issue_scope::MAIN_RED];
    let boxes = vec![issue("makgora", 912, "Run 1 failed", &main_red), issue("makgora", 913, "Nobody boxed this", &["bug"])];
    let survey = survey(boxes, hosts, accounts);
    let outcome = decide(&survey, "test");
    let named: Vec<String> = super::boxless(&survey, &outcome).iter().map(|issue| issue.reference().to_string()).collect();
    assert_eq!(named, ["makgora#913"]);
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

#[test]
fn a_run_that_began_before_the_proxy_paused_dispatch_launches_and_revives_nothing_after_it() {
    let (hosts, accounts) = fleet();
    let survey = survey(vec![issue("north", 757, "- [ ] a", &[])], hosts, accounts);
    let launch = &decide(&survey, "test").launches[0];
    let started = super::start(&survey, launch, false, || Some("claim rule not live".to_owned()));
    assert_eq!(started.line(), "skip north#757: dispatch was paused while this run was working: claim rule not live");
    assert_eq!(started.stopped().as_deref(), Some(started.line()), "a launch the pause stopped is named, not counted as launched");
    let step = super::revive::Step::Revive(super::revive::Revive { name: "n-state-machines".into(), registry: "gex45".into(), host: "gex45".into(), account: "claude:a".into(), stop: crate::fleet::stranded::Stop::Capacity, lanes: vec!["lane".into()], ended_ms: 0, local: None });
    let revival = super::revive::Revival { steps: vec![step], ..Default::default() };
    let lines = super::revive(&survey, &revival, false, || Some("paused".to_owned()));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("skip revive n-state-machines on gex45 claude:a") && lines[0].ends_with("dispatch was paused while this run was working: paused"), "{lines:?}");
}

/// north#782 had three boxes and one worker at a time. An issue with no Workers: line is staffed box by box up to the free slots:on 2026-11-07 first tries on north#634 (33 open boxes) ticked the box they took and still ended Not done because the brief asked for every box.
#[test]
fn an_issue_that_allows_three_workers_staffs_its_free_boxes_up_to_the_cap_and_one_without_a_cap_staffs_every_free_box() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let body = "Workers: 3\n- [ ] the loss is classified\n- [ ] the run row names its box\n- [ ] the staffing replays\n- [ ] the docs say so";
    let surveyed = |body: &str, held: &[&str]| {
        let mut survey = survey(vec![issue("makgora", 782, body, &[])], hosts.clone(), accounts.clone());
        survey.box_holds.insert(super::issues::IssueRef::new("makgora", 782), held.iter().map(|slug| ((*slug).to_owned(), "a worker".to_owned())).collect());
        survey
    };
    let boxes = |survey: &Survey| decide(survey, "test").launches.iter().map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect::<Vec<_>>();
    assert_eq!(boxes(&surveyed(body, &[])), [Some("the loss is classified".to_owned()), Some("the run row names its box".to_owned()), Some("the staffing replays".to_owned())]);
    assert_eq!(boxes(&surveyed(body, &["the-loss-is-classified"])), [Some("the run row names its box".to_owned()), Some("the staffing replays".to_owned())], "a box a worker holds is not staffed again, and the cap counts it");
    let full = decide(&surveyed(body, &["the-loss-is-classified", "the-run-row-names-its-box", "the-staffing-replays"]), "test");
    assert!(full.launches.is_empty());
    assert!(full.skipped.iter().any(|(_, why)| why.contains("3 workers hold its boxes, which is its cap of 3")), "{:?}", full.skipped);
    assert_eq!(boxes(&surveyed("- [ ] a\n- [ ] b", &[])), [Some("a".to_owned()), Some("b".to_owned())], "an issue with no Workers: line gets one launch per free box, each brief's Done when that box and not the whole issue (north#706, north#835)");
    assert_eq!(boxes(&surveyed("- [ ] a\n- [ ] b", &["a"])), [Some("b".to_owned())], "a held box holds only itself");
    let parallel = decide(&surveyed("- [ ] a\n- [ ] b", &[]), "test");
    let names: Vec<String> = parallel.launches.iter().map(|launch| super::effects::session_name(launch, 0.0)).collect();
    assert_eq!(names.len(), 2, "{:?}", parallel.skipped);
    assert_ne!(names[0], names[1], "two boxes of one issue launch under two names, or the launcher refuses the second");
    let long = decide(&surveyed("- [ ] the machine refusal detector reads the refused machine and files one box per cause", &[]), "test");
    let name = super::effects::session_name(&long.launches[0], 0.0);
    let slug = crate::threads::claims::box_slug("the machine refusal detector reads the refused machine and files one box per cause").unwrap();
    assert_eq!(name, format!("sc-782-d0000-{slug}"));
    assert!(name.len() > 40, "a box-named session keeps its whole slug however long it is: {name}");
    assert_eq!(boxes(&surveyed("- [ ] a\n- [ ] a", &[])), [Some("a".to_owned())], "one box line written twice is still one box, so it is launched once");
    let only = issue("makgora", 782, "- [ ] a", &[]);
    let twice_read = survey(vec![only.clone(), only], vec![host("gex45", 2, Ok(0))], accounts.clone());
    let once = decide(&twice_read, "test");
    assert_eq!(once.launches.len(), 1, "one ready box and two free slots launches once: {:?}", once.skipped);
    let names: BTreeSet<String> = once.launches.iter().map(|launch| super::effects::session_name(launch, 0.0)).collect();
    assert_eq!(names.len(), 1, "a run never collides with a name it just created: {names:?}");
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
    assert_eq!(boxes(&tried).len(), 4, "a box tried twice waits for the proxy alone, and its issue's other boxes still launch: {:?}", tried.skipped);
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

#[test]
fn a_live_reading_box_is_not_staffed_and_a_box_gets_one_launch() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let scoreboard = "`north ledger scoreboard` shows landing queue p90 over 1 h at or below 10 min in two separate hours";
    let dashboard = "`north ledger scoreboard` shows first tries lost to the harness over 3 h at or below 2% Done when: `north dash --bars --all | rg -q 'first tries? lost.*PASS'` exits 0";
    let reuse = "Verdict reuse shadow: `north land reuse report --since 7d` on the lander host prints at least 100 landings and 0 disagreements.";
    let journal = "The laptop's hourly lane-gc runs under 2 CPU minutes once the fix is live. Done when: `journalctl --user -u lane-gc.service --since -3h -o cat | rg Consumed | tail -2` exits 0";
    let plain = "the cache keeps the newest row";
    let boxes = vec![
        issue("north", 902, &format!("- [ ] {scoreboard}\n- [ ] {dashboard}\n- [ ] {reuse}\n- [ ] {plain}"), &[]),
        issue("north", 1028, &format!("- [ ] {journal}"), &[]),
    ];
    let outcome = decide(&survey(boxes, hosts, accounts), "test");
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["north#902"], "the live-only issue launches nothing, not even a bare brief");
    let boxes: Vec<String> = outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect();
    assert_eq!(boxes, [plain], "a live reading is not staffed and the plain box launches once");
    let skipped: Vec<&str> = outcome.skipped.iter().map(|(_, why)| why.as_str()).collect();
    assert!(skipped.iter().any(|why| why.contains("only open box waits until it reads a live journal")), "{skipped:?}");
}

#[test]
fn a_box_placed_on_the_laptop_is_queued_through_the_north_binary_not_a_threads_program_on_path() {
    let (hosts, accounts) = fleet();
    let survey = survey(Vec::new(), hosts, accounts);
    let route = super::tier::Route { tier: super::tier::Tier::Sonnet, category: "feature".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let launch = super::plan::Launch { item: super::issues::IssueRef { repo: "north".into(), number: 742 }, host: "whiterabbit".into(), account: "claude:a".into(), brief: "brief".into(), route, local: None, lineage: super::plan::Lineage::default() };

    let (program, words) = super::effects::command(&launch, "north-742-d1756", &survey, "/home/tom", "/runtime/bin/north").unwrap();

    assert_eq!(program, "/runtime/bin/north");
    assert_eq!(words[..7], ["threads", "queue", "add", "laptop", "north#742", "--", "north"]);
    assert_eq!(words[7..15], ["fleet", "dispatch", "queued", "--by", "nexus", "--item", "north#742", "--"]);
}

/// north#888, reversed by north#1054: on 2026-11-13 dispatch skipped three issues whose claim slug matched no box of their body, each holding every open box; an unmatched claim is stale and holds none.
#[test]
fn a_claim_on_an_unmatched_slug_is_stale_and_leaves_the_other_boxes_takeable() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let surveyed = |held: &str| {
        let mut survey = survey(vec![issue("north", 879, "- [ ] the tax is counted\n- [x] the tax is named\n- [ ] the docs say so", &[])], hosts.clone(), accounts.clone());
        survey.box_holds.insert(super::issues::IssueRef::new("north", 879), [(held.to_owned(), "fleet session n882-tax on nexus".to_owned())].into());
        decide(&survey, "test")
    };
    let named = surveyed("the-tax-is-counted");
    assert_eq!(named.launches.len(), 1, "a claim on a box holds only that box: {:?}", named.skipped);
    let prefix = surveyed("the-tax");
    assert_eq!(prefix.launches.len(), 1, "a claim on a box's first words holds that box: {:?}", prefix.skipped);
    let unnamed = surveyed("n882-tax");
    assert_eq!(unnamed.launches.len(), 2, "a stale claim holds no box: {:?}", unnamed.skipped);
    assert!(unnamed.skipped.iter().any(|(_, why)| why.contains("stale claim on box `n882-tax`") && why.contains("fleet session n882-tax on nexus") && why.contains("names no box")), "{:?}", unnamed.skipped);
    assert_eq!(surveyed("the-tax-is-named").launches.len(), 2, "a ticked box a worker still holds is a box of the issue");
}

/// north#1054: on 2026-11-10 a session spent six turns on north#882's box before finding its check runs `north ops climb run --dry-run`, a verb north#970 deleted with the north-climb crate.
#[test]
fn a_box_whose_check_names_a_retired_verb_is_held() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let reference = "Commands:\n  ops dividend [--dry-run] [CHECK]  Evaluate\n  ops strategy replay [--since 24h]  Decide\n  fleet dispatch [--dry-run]  Place\n  threads ARGV  Run\n";
    let live = "the dispatch table lists each box when `north fleet dispatch --dry-run` runs";
    let verb = "the week's loss prints when `north ops climb run --dry-run` runs";
    let path = "arms are read in crates/north-climb/src/arms.rs";
    let body = format!("- [ ] {live}\n- [ ] {verb}\n- [ ] {path}");
    let mut survey = survey(vec![issue("north", 872, &body, &[])], hosts, accounts);
    let history = "-  ops climb run --dry-run [--since 7d]  Print the loss\n";
    survey.mainline = super::retired::Mainline { verbs: Some(north_cli_reference::Verbs::parse(reference)), removed: Some(north_cli_reference::Verbs::removed_in(history)), gone: BTreeMap::from([("north".to_owned(), BTreeSet::from(["crates/north-climb/src/arms.rs".to_owned()]))]) };
    let outcome = decide(&survey, "test");
    let boxes: Vec<String> = outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect();
    assert_eq!(boxes, [live], "only the box whose check main still has launches");
    let held: Vec<&String> = outcome.skipped.iter().map(|(_, why)| why).collect();
    assert!(held.len() == 2 && held[0].contains("check retired: north ops climb") && held[1].contains("check retired: crates/north-climb/src/arms.rs"), "{held:?}");
}

/// On 2026-11-10 at 23:06 UTC nexus held 17 ready boxes as `check retired` because their checks name `north concerns` and `north project`, verbs those boxes exist to add, which the reference never listed.
#[test]
fn a_box_whose_check_names_a_verb_main_never_had_is_placed_and_one_a_landed_commit_removed_is_held() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let reference = "Commands:\n  fleet dispatch [--dry-run]  Place\n";
    let history = "--- a/src/cli/reference.txt\n+++ b/src/cli/reference.txt\n-  scoreboard [--record]  Print the numbers\n";
    let adds = "the verb prints a table when `north concerns list` runs";
    let removed = "the numbers print when `north scoreboard --record` runs";
    let body = format!("- [ ] {adds}\n- [ ] {removed}");
    let mut survey = survey(vec![issue("north", 709, &body, &[])], hosts, accounts);
    survey.mainline = super::retired::Mainline { verbs: Some(north_cli_reference::Verbs::parse(reference)), removed: Some(north_cli_reference::Verbs::removed_in(history)), gone: BTreeMap::new() };
    let outcome = decide(&survey, "test");
    let boxes: Vec<String> = outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect();
    assert_eq!(boxes, [adds], "{:?}", outcome.skipped);
    assert!(outcome.skipped.iter().any(|(_, why)| why.contains("check retired: north scoreboard")), "{:?}", outcome.skipped);
}

/// north#1210: restricted to the outside provider, a round staffed it on any box its subject allowed and named Claude routing for every box it refused.
#[test]
fn a_dispatch_round_places_only_on_the_listed_providers_draws_only_the_band_there_and_never_uses_an_excluded_host() {
    use super::deepseek::{Seen, Settings};
    let hosts = vec![host("nexus", 6, Ok(0)), host("whiterabbit", 8, Ok(0))];
    let accounts = vec![account("claude:a", &["nexus", "whiterabbit"], 80.0)];
    let band = "Spec: exact\nScope: module";
    let boxes = vec![
        issue("north", 1, &format!("{band}\n- [ ] add a column"), &[]),
        issue("north", 2, &format!("{band}\n- [ ] narrow the sandbox reads"), &[]),
        issue("north", 3, "Spec: exact\nScope: cross-module\n- [ ] a third column", &[]),
        issue("galileo", 4, &format!("{band}\n- [ ] a galileo column"), &[]),
        issue("north", 5, "Category: debugging-unknown-cause\nClass: a default fills in an undeclared facet\nSpec: exact\nScope: module\n- [ ] a fifth column", &[]),
        issue("north", 6, "Spec: measured\nScope: module\n- [ ] a sixth column", &[]),
        issue("north", 7, "Scope: module\n- [ ] a seventh column", &[]),
        issue("north", 8, "Spec: exact\n- [ ] an eighth column", &[]),
    ];
    let mut planned = survey(boxes, hosts, accounts);
    let deepseek: toml_edit::DocumentMut = "[dispatch]\nhosts = [\"whiterabbit\", \"nexus\"]\nslot_cap = 4\nclaude_only = ['\\bsandbox']\n".parse().unwrap();
    planned.deepseek = Seen { settings: Settings::parse(&deepseek), ..Seen::default() };

    let open = decide(&planned, "test");
    assert!(open.launches.iter().any(|launch| launch.account.starts_with("claude:")), "unrestricted, the sandbox box goes to Claude");
    assert!(open.launches.iter().any(|launch| launch.host == "whiterabbit"), "unrestricted, the laptop takes work");

    planned.cfg = format!("dispatch_providers = [\"opencode\"]\ndispatch_hosts_excluded = [\"whiterabbit\"]\n{CONFIG}").parse().unwrap();
    let scoped = decide(&planned, "test");

    let placed: Vec<(String, &str, &str)> = scoped.launches.iter().map(|launch| (launch.item.to_string(), launch.account.as_str(), launch.host.as_str())).collect();
    assert_eq!(placed, [("north#1".to_owned(), "opencode:go", "nexus")], "{:?}", scoped.skipped);
    let armed = &scoped.launches[0];
    assert!(armed.brief.contains("Arm: deepseek-") && armed.route.cell.is_none() && armed.route.cell_role.is_none(), "the outside provider's launch is its own arm on no Claude cell: {:?}", armed.route);
    let why = |item: &str| scoped.skipped.iter().find(|(skipped, _)| skipped.to_string() == item).map(|(_, why)| why.clone()).unwrap_or_default();
    assert!(why("north#2").contains("its subject matches") && why("north#2").contains("outside dispatch_providers = [opencode]"), "{}", why("north#2"));
    assert!(why("north#3").contains("Scope: cross-module is wider than one module"), "{}", why("north#3"));
    assert!(why("north#5").contains("Category: debugging-unknown-cause is outside the band"), "{}", why("north#5"));
    assert!(why("north#6").contains("Spec: measured is outside the band"), "{}", why("north#6"));
    assert!(why("north#7").contains("Spec: not declared"), "{}", why("north#7"));
    assert!(why("north#8").contains("Scope: not declared"), "{}", why("north#8"));
    assert!(why("galileo#4").contains("galileo is not a public repository"), "{}", why("galileo#4"));
    let lines = super::scope::Scope::parse(&planned.cfg).lines();
    assert_eq!(lines, ["dispatch providers: opencode".to_owned(), "dispatch hosts excluded: whiterabbit".to_owned()]);
}

/// north#1187: on 2026-11-12 a dry-run planned a launch on an issue one of whose boxes an open gate with no default held, and named neither the box nor a skip.
#[test]
fn a_box_held_by_an_open_gate_with_no_default_is_skipped_with_a_line_naming_the_gate_and_every_launch_names_its_box() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let mut survey = survey(vec![issue("north", 1187, "- [ ] the first box is built\n- [ ] the second box is built", &[])], hosts, accounts);
    let listing = serde_json::json!([{"id": 7, "session": "north-1187-x", "question": "Which?", "closed_ms": null, "default_at_ms": null, "issue": "north#1187", "box": "second box", "host": "gex45"}]);
    let rows = super::survey::gate_rows(&listing, "gex45");
    super::survey::gate_holds(rows.into_iter(), &survey.issues, 0.0, &mut survey.box_holds);
    let outcome = decide(&survey, "test");
    let boxes: Vec<String> = outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect();
    assert_eq!(boxes, ["the first box is built"], "{:?}", outcome.skipped);
    assert!(outcome.skipped.iter().any(|(_, why)| why.contains("the second box is built") && why.contains("open north gate gex45:7")), "{:?}", outcome.skipped);
}

#[test]
fn dispatch_staffs_open_leaf_issues_under_their_parent() {
    use super::issues::{Children, ParentRef};
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let check = "Done when: `cargo nextest run -p north-dispatch` exits 0";
    let under = |number: u64, parent: u64| Issue { parent: Some(ParentRef::new("tompassarelli", "north", parent)), ..issue("north", number, check, &[]) };
    let with_children = |mut issue: Issue, total: u64, converted: u64| {
        issue.children = Children { total, completed: 0, converted };
        issue
    };
    let issues = vec![
        with_children(issue("north", 1, "The outcome the leaves below make true.", &[]), 1, 1),
        with_children(under(2, 1), 1, 1),
        under(3, 2),
        with_children(Issue { parent: Some(ParentRef::new("tompassarelli", "north", 1)), ..issue("north", 4, "- [ ] a box of an issue that has a sub-issue with no checkbox", &[]) }, 2, 1),
        with_children(Issue { parent: Some(ParentRef::new("tompassarelli", "north", 1)), ..issue("north", 5, "- [ ] a box of an issue whose sub-issues all hold boxes", &[]) }, 1, 0),
    ];
    let outcome = decide(&survey(issues, hosts, accounts), "test");
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["north#3", "north#5"], "the open leaf and the box of an issue whose sub-issues all hold boxes are staffed: {:?}", outcome.skipped);
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
