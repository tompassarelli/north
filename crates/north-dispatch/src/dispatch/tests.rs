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
    let spent = |account: &Account| spend::Account { name: account.name.clone(), plan_week: account.plan.week, five_hour: window(account.room.five_hour, 5.0), week: window(account.room.week, account.room.week_reset_h), counted: true };
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
    let world = super::issues::World { open: &open, holds: &holds, live: &live, planned: &planned, backlog: None, at: 0.0 };
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
