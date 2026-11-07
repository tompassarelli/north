use super::issues::Issue;
use super::plan::Cloud;
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
    Host { name: name.into(), cap: Some(cap), running: running.map_err(str::to_owned), measure: None, builds: None }
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
    let spent = |account: &Account| spend::Account { name: account.name.clone(), plan_week: account.plan.week, five_hour: window(account.room.five_hour, 5.0), week: window(account.room.week, account.room.week_reset_h), counted: true, turn: None };
    View { at: 0.0, accounts: accounts.iter().map(spent).collect(), working: 0, trips: Vec::new(), keyed: Vec::new() }
}

/// The shipped rules with a cap no test fleet reaches, so these tests pin slots and order alone.
pub(super) fn rules() -> Rules {
    Rules { cap: 1000, ..Rules::configured() }
}

pub(super) fn issue(repo: &str, number: u64, body: &str, labels: &[&str]) -> Issue {
    Issue { repo: repo.into(), number, title: format!("box {number}"), body: format!("{body}\nCategory: feature"), labels: labels.iter().map(|label| (*label).to_owned()).collect(), ..Issue::default() }
}

pub(super) fn survey(issues: Vec<Issue>, cloud: Cloud, hosts: Vec<Host>, accounts: Vec<Account>) -> Survey {
    let open = issues.iter().map(Issue::reference).collect();
    Survey {
        here: "nexus".into(),
        cfg: CONFIG.parse().unwrap(),
        projects: super::projects::Projects::read(&CONFIG.parse().unwrap()).unwrap(),
        hosts,
        spend: view(&accounts),
        rules: rules(),
        accounts,
        issues,
        open,
        holds: BTreeMap::new(),
        box_holds: BTreeMap::new(),
        box_tries: BTreeMap::new(),
        live: BTreeSet::new(),
        listed: BTreeSet::new(),
        cloud,
        laptop: Some("whiterabbit".into()),
        cloud_running: 0,
        notes: Vec::new(),
        frozen: Vec::new(),
        history: super::tier::History::default(),
        ledger: BTreeMap::new(),
        grounds: super::grounds::Ledger::default(),
        strands: Vec::new(),
        saturated: BTreeMap::new(),
        revived: BTreeMap::new(),
        backlog: None,
        fresh_waits: Vec::new(),
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
fn a_saturated_host_takes_nothing_so_a_code_box_places_on_the_next_host_and_a_native_box_waits_for_a_native_host_and_each_launch_takes_a_session_and_a_build_from_its_host() {
    super::spread_tests::a_host_whose_admission_queue_is_at_its_limit_or_whose_root_disk_is_past_the_reapers_eviction_line_takes_no_session();
    super::spread_tests::a_host_whose_pushed_github_token_is_past_its_trust_takes_no_session_and_its_box_places_on_another_host();
    super::spread_tests::launches_earlier_in_the_run_count_against_each_hosts_sessions_and_admission_queue();
    let (mut hosts, accounts) = fleet();
    let queued = "gex45 skipped: 14.0 GiB free on the root disk, under 15";
    hosts[0] = hosts[0].clone().saturated(Some(queued.into()));
    let boxes = vec![issue("makgora", 50, "- [ ] a", &[]), issue("makgora", 51, "- [ ] b", &["needs:native-pair"])];
    let outcome = decide(&survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts), "test");
    let launched: Vec<(String, &str)> = outcome.launches.iter().map(|launch| (launch.item.to_string(), launch.host.as_str())).collect();
    assert_eq!(launched, [("makgora#50".to_owned(), "nexus")]);
    let (item, why) = outcome.skipped.last().unwrap();
    assert_eq!(item.to_string(), "makgora#51");
    assert!(why.contains(&format!("gex45 takes nothing: {queued}")), "{why}");
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
    let mut paused = survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts);
    paused.projects = super::projects::Projects::read(&cfg).unwrap();
    assert_eq!(paused.projects.set_aside(), ["galileo", "makgora", "wisp"]);
    let outcome = decide(&paused, "test");
    let launched: Vec<String> = outcome.launches.iter().map(|launch| launch.item.to_string()).collect();
    assert_eq!(launched, ["north#1"]);
    let skipped: Vec<(String, &str)> = outcome.skipped.iter().map(|(item, why)| (item.to_string(), why.as_str())).collect();
    assert_eq!(skipped, [("makgora#798".to_owned(), "project makgora paused paused = true"), ("wisp#230".to_owned(), "project wisp paused paused = true")]);
}

#[test]
fn the_main_red_workflows_boxless_issue_is_neither_named_for_a_box_nor_given_a_clean_up_group() {
    let (hosts, accounts) = fleet();
    let main_red = ["bug", "priority:now", "evidence:code", crate::platform::cleanup::MAIN_RED];
    let boxes = vec![issue("makgora", 912, "Run 1 failed", &main_red), issue("makgora", 913, "Nobody boxed this", &["bug"])];
    let survey = survey(boxes, Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts);
    let outcome = decide(&survey, "test");
    let named: Vec<String> = super::boxless(&survey, &outcome).iter().map(|issue| issue.reference().to_string()).collect();
    assert_eq!(named, ["makgora#913"]);
    assert!(!crate::platform::cleanup::applies(&main_red.map(str::to_owned)));
}

#[test]
fn a_dispatcher_with_no_api_key_places_on_the_host_that_holds_one_when_every_plan_is_under_the_reserve() {
    let (hosts, _) = fleet();
    let low = Room { five_hour: 90.0, week: 5.0, week_reset_h: 100.0 };
    let plans = vec![sized("claude:a", &["nexus"], MAX_5X, low), sized("claude:b", &["gex45"], MAX_5X, low)];
    let policy: toml_edit::DocumentMut = "[accounts.claude.api_orgs.proton]\nid = \"org\"\nkeys = { nexus = [\"/run/secrets/key\"], gex45 = [\"/run/secrets/key\"] }\n".parse().unwrap();
    let source = north_strategy_types::placement::GrantSource { account: "claude:api:proton".into(), left_usd: 500.0, deadline_h: 200.0, urgent: true, line: String::new() };
    let grants = super::survey::grants(&policy, &crate::fleet::roster::Roster::default(), "nexus", vec![source]);
    assert_eq!(grants.iter().map(|grant| (grant.name.as_str(), grant.hosts.clone())).collect::<Vec<_>>(), [("claude:api:proton", vec!["gex45".to_owned()])], "nexus dispatch dropped every grant because its own roster holds no api key (north#723)");

    let mut surveyed = survey(vec![issue("makgora", 7, "- [ ] a", &[])], Cloud::Closed("no credit".into()), hosts, plans.iter().cloned().chain(grants).collect());
    surveyed.spend = View { keyed: vec!["claude:api".into()], ..view(&plans) };
    let outcome = decide(&surveyed, "test");
    assert_eq!(outcome.launches.iter().map(|launch| (launch.host.as_str(), launch.account.as_str())).collect::<Vec<_>>(), [("gex45", "claude:api:proton")], "{:?}", outcome.skipped);
}

#[test]
fn a_run_that_began_before_the_proxy_paused_dispatch_launches_and_revives_nothing_after_it() {
    let (hosts, accounts) = fleet();
    let survey = survey(vec![issue("north", 757, "- [ ] a", &[])], Cloud::Unreachable("the credit login is on nexus".into()), hosts, accounts);
    let launch = &decide(&survey, "test").launches[0];
    let (line, failure) = super::start(&survey, launch, false, || Some("claim rule not live".to_owned()));
    assert_eq!(line, "skip north#757: dispatch was paused while this run was working: claim rule not live");
    assert_eq!(failure, None);
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
        let mut survey = survey(vec![issue("makgora", 782, body, &[])], Cloud::Unreachable("the credit login is on nexus".into()), hosts.clone(), accounts.clone());
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
}

/// north#835: on 2026-11-08 merging issues left 14 open boxes on north#719 and 28 on north#706, run one at a time; north-740-d1801 spent a first try on a box whose window over readings had not passed.
#[test]
fn independent_boxes_launch_side_by_side_while_an_overlapping_a_tried_and_a_windowed_box_wait() {
    let (_, accounts) = fleet();
    let hosts = vec![host("gex45", 24, Ok(0))];
    let first = "the claims replay in `crates/north-fleet/src/fleet/claims.rs` passes";
    let body = format!("- [ ] {first}\n- [ ] holds in crates/north-dispatch/src/dispatch/hotspots.rs\n- [ ] harness/skills/workers/SKILL.md says so\n- [ ] the ready list in `crates/north-dispatch/src/dispatch` names each box\n- [ ] Over 3 h no launch is refused");
    let surveyed = |tried: &[&str]| {
        let mut survey = survey(vec![issue("north", 835, &body, &[])], Cloud::Unreachable("the credit login is on nexus".into()), hosts.clone(), accounts.clone());
        survey.box_tries.insert(super::issues::IssueRef::new("north", 835), tried.iter().filter_map(|text| Some((crate::threads::claims::box_slug(text)?, "tried twice".to_owned()))).collect());
        decide(&survey, "test")
    };
    let boxes = |outcome: &super::Outcome| outcome.launches.iter().filter_map(|launch| super::facets::declared_in(&launch.brief, "Box").map(str::to_owned)).collect::<Vec<_>>();
    let outcome = surveyed(&[]);
    assert_eq!(boxes(&outcome), [first, "holds in crates/north-dispatch/src/dispatch/hotspots.rs", "harness/skills/workers/SKILL.md says so"], "the windowed box waits out its 3 h");
    let held: Vec<&String> = outcome.skipped.iter().map(|(_, why)| why).collect();
    assert!(held.len() == 1 && held[0].contains("box `the ready list") && held[0].contains("both name crates/north-dispatch/src/dispatch"), "{held:?}");
    let tried = surveyed(&[first]);
    assert_eq!(boxes(&tried).len(), 2, "a box tried twice waits for the proxy alone, and its issue's other boxes still launch: {:?}", tried.skipped);
}

/// The 2026-11-07 3:15 PM reading: claude:a at 99% of its week took two launches and both died on the weekly limit (north#259).
#[test]
fn two_boxes_at_a_week_reading_of_99_percent_place_on_the_account_that_covers_a_turn_and_hold_when_none_does_here() {
    let hosts = vec![host("gex45", 24, Ok(22))];
    let spent = Room { five_hour: 90.0, week: 1.0, week_reset_h: 100.0 };
    let both = vec![sized("claude:a", &["gex45"], MAX_5X, spent), sized("claude:b", &["gex45"], MAX_5X, Room { five_hour: 90.0, week: 60.0, week_reset_h: 100.0 })];
    let boxes = || vec![issue("north", 790, "- [ ] a", &[]), issue("north", 634, "- [ ] b", &[])];
    let covered = |accounts: Vec<Account>| {
        let mut surveyed = survey(boxes(), Cloud::Closed("no credit".into()), hosts.clone(), accounts);
        surveyed.spend.accounts.iter_mut().for_each(|account| account.turn = Some(spend::Turn::measured(8.0, 66, 0.2, 0.5)));
        decide(&surveyed, "test")
    };
    let placed = covered(both.clone());
    assert_eq!(placed.launches.iter().map(|launch| launch.account.as_str()).collect::<Vec<_>>(), ["claude:b", "claude:b"], "{:?}", placed.skipped);
    let elsewhere = sized("claude:c", &["nexus"], MAX_5X, Room { five_hour: 90.0, week: 60.0, week_reset_h: 100.0 });
    let held = covered(vec![both[0].clone(), elsewhere]);
    assert!(held.launches.is_empty());
    let why = format!("{:?}", held.skipped);
    assert!(why.contains("claude:a has 1.0% of its week window left and one worker turn takes 4.0% at p90"), "{why}");
}

// KEEP(bug): every laptop launch from nexus failed for six hours on 2026-11-07 because the dispatch unit's PATH has no `threads` shim (north#828)
#[test]
fn a_box_placed_on_the_laptop_is_queued_through_the_north_binary_not_a_threads_program_on_path() {
    let (hosts, accounts) = fleet();
    let survey = survey(Vec::new(), Cloud::Unreachable(String::new()), hosts, accounts);
    let route = super::tier::Route { tier: super::tier::Tier::Sonnet, category: "feature".into(), effort: None, effort_arm: false, follows: None, why: String::new() };
    let launch = super::plan::Launch { item: super::issues::IssueRef { repo: "north".into(), number: 742 }, host: "whiterabbit".into(), account: "claude:a".into(), brief: "brief".into(), route, cloud: false, local: None };

    let (program, words) = super::effects::command(&launch, "north-742-d1756", &survey, "/home/tom", "/runtime/bin/north").unwrap();

    assert_eq!(program, "/runtime/bin/north");
    assert_eq!(words[..7], ["threads", "queue", "add", "laptop", "north#742", "--", "north"]);
}
