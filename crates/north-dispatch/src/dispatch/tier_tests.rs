use super::plan::Cloud;
use super::tests::{issue, survey};
use super::{decide, slots};

fn uncategorised(repo: &str, number: u64, body: &str, labels: &[&str]) -> super::issues::Issue {
    super::issues::Issue { body: body.into(), ..issue(repo, number, "", labels) }
}


#[test]
fn a_rolling_alarm_issue_is_not_ready_because_its_body_declares_no_category_while_the_same_body_with_one_launches() {
    let rise = crate::ledger::rises::Rise { tier: "sonnet".into(), category: "tooling".into(), before: (5.2, 3), after: (10.4, 3), boxes: Vec::new() };
    let body = crate::ledger::rises::issue_body(&[rise]).unwrap();
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), room: None, running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, grant: None }];
    let alarm = decide(&survey(vec![uncategorised("north", 608, &body, &[])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()), "test");
    assert!(alarm.launches.is_empty() && alarm.ready.is_empty(), "{:?}", alarm.skipped);
    assert_eq!(alarm.skipped, [(super::issues::IssueRef::parse("north#608").unwrap(), super::facets::UNCATEGORISED.to_owned())]);

    let headed = decide(&survey(vec![uncategorised("north", 608, &format!("Category: tooling\n{body}"), &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert_eq!(headed.launches.len(), 1, "the header alone is what staffed north#608: {:?}", headed.skipped);
}

#[test]
fn every_launch_starts_at_its_category_tier_by_hand_as_in_dispatch_an_unknown_category_is_refused_and_effort_and_context_arms_draw_their_seeded_shares() {
    use super::cells::Cell;
    use super::tier::{CONTEXT_WINDOWS, Evidence, History, Tier, context_arm, route_for, seed_cell};
    use std::collections::BTreeMap;
    let facet = |category: &'static str| move |key: &str| if key == "Category" { category.to_owned() } else { super::facets::default(key).to_owned() };
    let unread = BTreeMap::new();
    let draw = |category: &'static str| (1..=1000).map(|n| route_for(&format!("north#{n}"), &facet(category), &History::default(), &unread).unwrap()).collect::<Vec<_>>();
    let routes = draw("review");
    assert!(
        routes.iter().all(|route| route.cell.is_some_and(|cell| super::cells::allowed("review").contains(&cell)) && route.cell_role.is_some() && route.stamp().is_none()),
        "{routes:?}"
    );
    let leaders = routes.iter().filter(|route| route.cell_role == Some("leader")).count();
    let challengers = routes.iter().filter(|route| route.cell_role == Some("challenger")).count();
    assert!(leaders > 0 && challengers > 0, "a category draws its leader and its challengers: {leaders} leaders, {challengers} challengers");
    assert_eq!(routes, draw("review"), "the draw is seeded by the item and its launch count");
    use crate::fleet::model::Effort;
    use north_providers::providers::claude::fleet::Claude;
    for category in crate::router::brief::CATEGORIES {
        for cell in super::cells::allowed(category) {
            let effort = Effort::parse(cell.effort).expect("an effort word");
            assert!(
                crate::fleet::registry::efforts::taken(&Claude, Some(cell.model), Some(effort), "seed").is_ok(),
                "{category}: dispatch may draw {} but the launcher refuses it",
                cell.name()
            );
        }
    }
    assert!(crate::fleet::registry::efforts::taken(&Claude, Some("claude-sonnet-5-5"), Some(Effort::Medium), "seed").is_ok_and(|effort| effort == Some(Effort::Medium)));
    assert!(crate::fleet::registry::efforts::taken(&Claude, Some("claude-sonnet-5-5"), Some(Effort::Xhigh), "seed").is_err(), "Sonnet xhigh stays excluded");
    let item = "north#7";
    let mut history = History::default();
    let first = route_for(item, &facet("review"), &history, &unread).unwrap();
    history.launched(item, "first", &first);
    assert!(first.why.contains(&format!("seeded cell:{item}#0")) && route_for(item, &facet("review"), &history, &unread).unwrap().why.contains(&format!("seeded cell:{item}#1")));

    let boxes = |usd: f64| vec![crate::router::settle::Outcome { usd, closed: true, kept: true }; 15];
    let evidence = |medium: f64, high: f64| Evidence { cell_boxes: BTreeMap::from([("opus-medium".to_owned(), boxes(medium)), ("opus-high".to_owned(), boxes(high))]), ..Evidence::default() };
    let ledger = |evidence: Evidence| BTreeMap::from([("review".to_owned(), evidence)]);
    let cheaper_high = route_for("north#1", &facet("review"), &History::default(), &ledger(evidence(12.0, 9.0))).unwrap();
    assert_eq!(cheaper_high.cell.map(Cell::name).as_deref(), Some("opus-high"));
    assert_eq!(route_for("north#1", &facet("review"), &History::default(), &ledger(evidence(9.0, 12.0))).unwrap().cell.map(Cell::name).as_deref(), Some("opus-medium"));
    let live = super::cells::live("review", seed_cell("review"), &ledger(evidence(12.0, 9.0))["review"].cell_boxes);
    assert_eq!(live.len(), super::cells::LIVE, "{live:?}");
    assert_eq!(live[0].cell.name(), "opus-high");
    assert!(live.iter().all(|live| live.cell.name() != "opus-medium"), "a challenger settled behind the leader leaves: {live:?}");

    let draw = || (1..=4000).map(|n| context_arm(&format!("north#{n}"), "docs-policy", &History::default(), &Evidence::default()).0).collect::<Vec<_>>();
    let arms = draw();
    for window in CONTEXT_WINDOWS {
        let arm = crate::fleet::compaction::arm(window);
        let share = arms.iter().filter(|drawn| **drawn == arm).count();
        assert!((850..=1150).contains(&share), "{arm}: {share} of 4000");
    }
    assert_eq!(arms, draw(), "the draw is seeded by the item and its launch count");

    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), room: None, running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, grant: None }];
    let launched = decide(&survey(vec![uncategorised("north", 636, "Category: docs-policy\n- [ ] a box", &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    let [launch] = launched.launches.as_slice() else { panic!("{:?}", launched.skipped) };
    let stamp = format!("Arm: {}", context_arm("north#636", "docs-policy", &History::default(), &Evidence::default()).0);
    let lines: Vec<&str> = launch.brief.lines().collect();
    let tier = lines.iter().position(|line| line.starts_with("Tier: ")).unwrap();
    assert_eq!(lines[tier + 1], stamp, "{}", launch.brief);
    assert_eq!(crate::fleet::compaction::window(&launch.brief).map(crate::fleet::compaction::arm), Some(stamp.trim_start_matches("Arm: ").to_owned()));
    use super::manual::{Ask, start};
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let sonnet = super::tier::Route {
        tier: Tier::Sonnet,
        category: "feature".into(),
        effort: Some(super::tier::Effort::Medium),
        effort_arm: false,
        cell: Some(Cell { model: "sonnet", effort: "high" }),
        cell_role: Some("leader"),
        signal: None,
        follows: None,
        why: "a sonnet-high arm whose launch was built with medium effort".into(),
    };
    assert_eq!(
        sonnet.invocation(&cfg).unwrap(),
        ["--model", "claude-sonnet-5-5", "--effort", "medium"],
        "a Sonnet launch passes the medium the launcher admits"
    );
    let xhigh = super::tier::Route { effort: Some(super::tier::Effort::Xhigh), ..sonnet };
    assert_eq!(xhigh.invocation(&cfg).unwrap(), ["--model", "claude-sonnet-5-5", "--effort", "high"], "a Sonnet launch never passes the xhigh the launcher refuses");
    let hand = |category: &str| {
        let brief = format!("Item: north#7\nCategory: {category}");
        start(&Ask { item: "north#7", brief: &brief, prompt: &format!("{brief}\n\nDo the box."), model: None, reason: None, worker: true, places: false }, "", &cfg, &History::default(), &unread, &|_| super::grounds::Grounds::default())
    };
    for (category, tier) in [("tooling", Tier::Sonnet), ("feature", Tier::Sonnet), ("bug-known-cause", Tier::Sonnet), ("docs-policy", Tier::Sonnet), ("debugging-unknown-cause", Tier::Opus), ("mechanical", Tier::Haiku)] {
        let picked = hand(category).unwrap().route;
        let dispatched = route_for("north#7", &facet(category), &History::default(), &unread).unwrap();
        assert_eq!(seed_cell(category).tier(), tier, "{category}: the staffing rule seeds its cell");
        assert_eq!((picked.tier, &picked.category), (dispatched.tier, &dispatched.category), "{category}: a hand launch starts where dispatch does");
        assert!(dispatched.cell.is_some_and(|cell| super::cells::allowed(category).contains(&cell)), "{category}: {dispatched:?}");
    }
    let refused = hand("bug").unwrap_err();
    assert!(refused.contains("`Category: bug` is no category") && refused.contains("bug-known-cause"), "{refused}");
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), room: None, running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, grant: None }];
    let skipped = decide(&survey(vec![uncategorised("north", 637, "Category: bug\n- [ ] a box", &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert!(skipped.launches.is_empty() && skipped.skipped.iter().any(|(_, why)| why.contains(&refused)), "{:?}", skipped.skipped);
}

#[test]
fn a_task_launch_without_a_model_draws_its_category_arm() {
    use super::manual::{Ask, start};
    use super::cells::Cell;
    use super::tier::{Effort, History, Tier, route_for};
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let unread = BTreeMap::new();
    let launched = |category: &str, model: Option<&str>, reason: Option<&str>| {
        let brief = format!("Item: north#991\nCategory: {category}\n\nStaff the box.");
        let ask = Ask { item: "north#991", brief: &brief, prompt: &brief, model, reason, worker: true, places: false };
        start(&ask, "", &cfg, &History::default(), &unread, &|_| super::grounds::Grounds::default()).unwrap()
    };
    let facet = |category: &str| {
        let category = category.to_owned();
        move |key: &str| if key == "Category" { category.clone() } else { super::facets::default(key).to_owned() }
    };

    let reviewed = launched("review", None, None);
    let drawn = route_for("north#991", &facet("review"), &History::default(), &unread).unwrap();
    assert_eq!((reviewed.route.tier, reviewed.route.effort), (drawn.tier, drawn.effort), "{:?}", reviewed.route);
    assert_eq!((reviewed.route.cell, reviewed.route.cell_role), (drawn.cell, drawn.cell_role));
    assert!(drawn.cell.is_some_and(|cell| super::cells::allowed("review").contains(&cell)) && drawn.cell_role.is_some());
    assert!(!drawn.effort_arm && drawn.stamp().is_none(), "{}", drawn.why);
    assert_eq!(reviewed.model, drawn.tier.model_effort(&cfg).unwrap().0);
    let head: Vec<&str> = reviewed.prompt.lines().take_while(|line| !line.is_empty()).collect();
    let tier_line = format!("Tier: {}", drawn.tier.name());
    assert!(head.contains(&tier_line.as_str()) && head.iter().any(|line| line.starts_with("Arm: context-")), "{head:?}");
    let entry = reviewed.entry("gex45", "claude:b", "n991-draw").unwrap();
    assert_eq!((entry.tier.as_deref(), entry.category.as_deref()), (Some(drawn.tier.name()), Some("review")), "{entry:?}");
    assert_eq!((entry.effort_arm.as_deref(), entry.effort.as_deref(), entry.cell.as_deref(), entry.cell_role.as_deref()), (None, drawn.effort.map(Effort::name), drawn.cell.map(Cell::name).as_deref(), drawn.cell_role), "{entry:?}");
    assert!(entry.hand && entry.launched && entry.detail.contains("draws "), "{}", entry.detail);

    let featured = launched("feature", None, None);
    let dispatched = route_for("north#991", &facet("feature"), &History::default(), &unread).unwrap();
    assert_eq!((featured.route.tier, featured.route.effort), (dispatched.tier, dispatched.effort));
    let entry = featured.entry("gex45", "claude:b", "n991-draw").unwrap();
    assert_eq!((entry.category.as_deref(), entry.effort.as_deref(), entry.effort_arm.as_deref(), entry.cell.as_deref(), entry.cell_role.as_deref()), (Some("feature"), dispatched.effort.map(Effort::name), None, dispatched.cell.map(Cell::name).as_deref(), dispatched.cell_role), "{entry:?}");
    assert!(entry.detail.contains("feature starts on sonnet-high") && entry.detail.contains("draws "), "{}", entry.detail);

    let overridden = launched("feature", Some("claude-opus-5-5"), Some("the proxy staffs this box on Opus"));
    assert_eq!(overridden.route.tier, Tier::Sonnet);
    let entry = overridden.entry("gex45", "claude:b", "n991-draw").unwrap();
    assert_eq!((entry.detail.as_str(), entry.tier.as_deref(), entry.effort_arm.as_deref()), ("tier override: the proxy staffs this box on Opus", None, None), "{entry:?}");
}

#[test]
fn a_hand_launch_on_an_issue_with_no_open_done_when_box_is_refused() {
    use super::manual::{Ask, start};
    use super::tier::History;
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let brief = "Item: north#928\nCategory: tooling";
    let launch = |body: &str| start(&Ask { item: "north#928", brief, prompt: &format!("{brief}\n\nDo the box."), model: None, reason: None, worker: true, places: false }, body, &cfg, &History::default(), &BTreeMap::new(), &|_| super::grounds::Grounds::default());
    let drift = "`north tune` keeps this issue while any tier and category's median ratio of actual to estimated minutes or cost over the last 7 days sits outside 0.67-1.5x.\n\n## haiku mechanical\n\n- minutes: actual over estimate median 0.40x p90 2.35x inside 78% (n 45)\n";
    let refused = launch(drift).unwrap_err();
    assert!(refused.contains("north#928") && refused.contains("no open Done-when box"), "{refused}");
    assert!(launch("Category: tooling\n- [ ] a box").is_ok(), "the same launch on one open box still starts");
}

#[test]
fn a_hand_launched_brief_with_no_arm_draws_the_context_arm_dispatch_would_and_launches_claude_with_its_auto_compact_window() {
    use super::manual::{Ask, start};
    use super::tier::{Evidence, History, context_arm};
    use crate::fleet::model::{Account, LaunchRequest};
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtime::{Opening, Runtime};
    use north_providers::providers::claude::fleet::Claude;
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let launch = |brief: &str, worker: bool| {
        let ask = Ask { item: "north#636", brief, prompt: brief, model: None, reason: None, worker, places: false };
        start(&ask, "", &cfg, &History::default(), &BTreeMap::new(), &|_| super::grounds::Grounds::default()).unwrap()
    };
    let scratch = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(scratch.path().join("home")).unwrap();
    std::fs::write(scratch.path().join("home/settings.json"), r#"{"hooks": {"PreToolUse": [{}]}}"#).unwrap();
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: None, api: None };
    let window = |prompt: &str| {
        let request = LaunchRequest {
            verification: false,
            gates_aside: false,
            container_shell: false,
            container_dir: None,
            name: "n636-arm".into(),
            provider: Claude.provider(),
            account: "a".into(),
            cwd: scratch.path().to_path_buf(),
            model: None,
            effort: None,
            prompt: prompt.to_owned(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: Some("proxy".into()),
            task_ref: Some("north#636".into()),
            claim_box: None,
            placement: None,
            machine: None,
            capacity_need: None,
            role: None,
            resume: None,
            compact_window: None,
            route: None,
                    };
        let spec = Claude.host(&account, &request, Opening::Fresh(None), "").unwrap();
        spec.env.into_iter().find(|(name, _)| name == "CLAUDE_CODE_AUTO_COMPACT_WINDOW").map(|(_, value)| value.to_string_lossy().into_owned())
    };

    let unarmed = launch("Item: north#636\nCategory: debugging-known-cause\nTier: worker\n\nGoal: price the cache.", true);
    let (arm, _) = context_arm("north#636", &unarmed.route.category, &History::default(), &Evidence::default());
    let lines: Vec<&str> = unarmed.prompt.lines().collect();
    let tier = lines.iter().position(|line| line.starts_with("Tier: ")).unwrap();
    assert_eq!(lines[tier + 1], format!("Arm: {arm}"), "{}", unarmed.prompt);
    assert!(unarmed.route.why.contains(&format!("context arm {arm} drawn at")), "{}", unarmed.route.why);
    assert_eq!(window(&unarmed.prompt), crate::fleet::compaction::window(&format!("Arm: {arm}")).map(|tokens| tokens.to_string()));

    let armed = launch("Item: north#636\nCategory: debugging-known-cause\nTier: worker\nArm: context-400k\n\nGoal: x", true);
    assert_eq!((armed.prompt.matches("Arm: ").count(), window(&armed.prompt).as_deref()), (1, Some("400000")));
    let lead = launch("Item: north#636\nCategory: debugging-known-cause\nTier: worker\n\nGoal: x", false);
    assert_eq!(lead.prompt.matches("Arm: ").count(), 1, "{}", lead.prompt);
    assert!(window(&lead.prompt).is_some());
}

#[test]
fn a_hand_launch_on_a_tier_override_carries_the_eta_and_budget_of_the_tier_it_runs_and_the_facets_its_brief_leaves_out() {
    use super::grounds::Grounds;
    use super::manual::{Ask, start};
    use super::tier::{History, Tier};
    use super::tuned::Figure;
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let cells = |route: &super::tier::Route| {
        let (eta, usd) = if route.tier == Tier::Opus { (102.0, 50.68) } else { (52.0, 19.02) };
        Grounds { eta_cell: Some(Figure { value: eta, date: "2026-11-08".into() }), budget_cell: Some(Figure { value: usd, date: "2026-11-08".into() }), ..Grounds::default() }
    };
    let brief = "Item: north#706\nCategory: feature\nTier: sonnet-high\nSpec: measured\n\nDo the box.";
    let ask = Ask { item: "north#706", brief, prompt: brief, model: Some("claude-opus-5-5"), reason: Some("the proxy staffs this box on Opus"), worker: true, places: false };
    let launched = start(&ask, "", &cfg, &History::default(), &BTreeMap::new(), &cells).unwrap();
    assert!(launched.overridden.is_some() && launched.route.tier == Tier::Sonnet, "{:?}", launched.route);
    let head: Vec<&str> = launched.prompt.lines().take_while(|line| !line.is_empty()).collect();
    assert!(head.iter().any(|line| line.starts_with("ETA: 102 min (tune cell, 2026-11-08")), "{head:?}");
    assert!(head.iter().any(|line| line.starts_with("Budget: E p90 $50.68 (tune cell, 2026-11-08")), "{head:?}");
    assert_eq!(head.iter().filter(|line| line.starts_with("Spec: ")).count(), 1, "a facet the brief names is kept once: {head:?}");
    assert!(head.contains(&"Spec: measured"), "a facet the brief names is kept: {head:?}");
    for facet in ["Scope: module", "Surface: ts", "Verify: local-test"] {
        assert!(head.contains(&facet), "{facet} missing: {head:?}");
    }
}

#[test]
fn an_unknown_cause_bug_naming_a_repro_starts_on_sonnet_and_escalates_once_to_opus_medium_and_planning_effort_follows_its_scope() {
    use super::tier::{Effort, History, Tier, route};
    use std::collections::BTreeMap;
    let unread = BTreeMap::new();
    let bug = |body: &str| super::issues::Issue { body: format!("Category: debugging-unknown-cause\n{body}\n- [ ] a box"), ..issue("north", 966, "", &[]) };
    let rung = |issue: &super::issues::Issue, history: &History| route(issue, history, &unread).map(|route| (route.tier, route.effort)).unwrap();
    let repro = bug("Fails under `cargo nextest run -p north-dispatch stops`.");
    let first = route(&repro, &History::default(), &unread).unwrap();
    assert!(first.cell.is_some_and(|cell| super::cells::allowed("debugging-unknown-cause").contains(&cell)), "{first:?}");
    assert!(first.why.contains("an unknown-cause bug naming repro") && first.why.contains("starts on sonnet-high"), "{}", first.why);
    let no_repro = route(&bug("Workers stall at random."), &History::default(), &unread).unwrap();
    assert!(no_repro.why.contains("naming no failing test or repro command starts on worker at medium effort"), "{}", no_repro.why);
    let passing = route(&bug("Workers stall at random.\nDone when: `cargo nextest run -p north-dispatch` passes"), &History::default(), &unread).unwrap();
    assert!(passing.why.contains("naming no failing test or repro command"), "a passing check is no repro: {}", passing.why);
    let sonnet = super::tier::Route { tier: Tier::Sonnet, category: "debugging-unknown-cause".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: None, why: String::new() };
    let mut history = History::default();
    history.launched("north#966", "sonnet-try", &sonnet);
    let second = route(&repro, &history, &unread).unwrap();
    assert_eq!((second.tier, second.effort, second.follows.as_deref()), (Tier::Opus, Some(Effort::Medium), Some("sonnet-try (worker-sonnet)")));
    history.launched("north#966", "opus-try", &second);
    assert_eq!(rung(&repro, &history), (Tier::Opus, Some(Effort::Medium)), "never a second Sonnet try");

    let plan = |scope: &str| super::issues::Issue { body: format!("Category: planning\nScope: {scope}\n- [ ] a box"), ..issue("north", 940, "", &[]) };
    let module = route(&plan("module"), &History::default(), &unread).unwrap();
    assert!(module.why.contains("planning at Scope: module stays in one module, so worker at medium effort"), "{}", module.why);
    let spanning = route(&plan("cross-module"), &History::default(), &unread).unwrap();
    assert!(spanning.why.contains("planning at Scope: cross-module spans modules, so worker at high effort"), "{}", spanning.why);
    assert!(module.cell.is_some_and(|cell| super::cells::allowed("planning").contains(&cell)) && spanning.cell.is_some_and(|cell| super::cells::allowed("planning").contains(&cell)));
}

#[test]
fn a_hand_launch_with_a_model_draws_a_context_window() {
    use super::grounds::Grounds;
    use super::manual::{Ask, start, untasked_window};
    use super::tier::{CONTEXT_WINDOWS, History};
    use std::collections::BTreeMap;
    let cfg: toml_edit::DocumentMut = super::tests::CONFIG.parse().unwrap();
    let brief = "Item: north#636\nCategory: feature\nSpec: measured\n\nDo the box.";
    let launch = |model: &str, worker: bool| {
        let ask = Ask { item: "north#636", brief, prompt: brief, model: Some(model), reason: Some("the proxy staffs this box on that model"), worker, places: false };
        start(&ask, "", &cfg, &History::default(), &BTreeMap::new(), &|_| Grounds::default()).unwrap()
    };
    for (model, worker) in [("claude-opus-5-5", true), ("claude-sonnet-5-5", true), ("claude-opus-5-5", false)] {
        let launched = launch(model, worker);
        let arms: Vec<&str> = launched.prompt.lines().filter(|line| line.starts_with("Arm: ")).collect();
        assert_eq!(arms.len(), 1, "{model} worker={worker}: {}", launched.prompt);
        assert!(crate::fleet::compaction::window(&launched.prompt).is_some_and(|window| CONTEXT_WINDOWS.contains(&window)), "{}", launched.prompt);
    }
    let window = untasked_window("the-proxy", "feature", &History::default(), &BTreeMap::new());
    assert!(window.is_some_and(|window| CONTEXT_WINDOWS.contains(&window)), "{window:?}");
}
