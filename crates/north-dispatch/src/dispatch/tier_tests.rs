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
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, grant: None }];
    let alarm = decide(&survey(vec![uncategorised("north", 608, &body, &[])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()), "test");
    assert!(alarm.launches.is_empty() && alarm.ready.is_empty(), "{:?}", alarm.skipped);
    assert_eq!(alarm.skipped, [(super::issues::IssueRef::parse("north#608").unwrap(), super::facets::UNCATEGORISED.to_owned())]);

    let headed = decide(&survey(vec![uncategorised("north", 608, &format!("Category: tooling\n{body}"), &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert_eq!(headed.launches.len(), 1, "the header alone is what staffed north#608: {:?}", headed.skipped);
}

#[test]
fn every_launch_starts_at_its_category_tier_by_hand_as_in_dispatch_an_unknown_category_is_refused_and_effort_and_context_arms_draw_their_seeded_shares() {
    use super::tier::{CONTEXT_WINDOWS, Effort, Evidence, History, Tier, Trial, context_arm, route_for};
    use std::collections::BTreeMap;
    let facet = |category: &'static str| move |key: &str| if key == "Category" { category.to_owned() } else { super::facets::default(key).to_owned() };
    let unread = BTreeMap::new();
    let draw = |category: &'static str| (1..=1000).map(|n| route_for(&format!("north#{n}"), &facet(category), &History::default(), &unread).unwrap()).collect::<Vec<_>>();
    for category in ["planning", "review"] {
        let routes = draw(category);
        let xhigh = routes.iter().filter(|route| route.effort == Some(Effort::Xhigh)).count();
        assert!((450..=550).contains(&xhigh), "{category}: {xhigh} of 1000 drew xhigh");
        assert!(routes.iter().all(|route| route.tier == Tier::Opus && matches!(route.effort, Some(Effort::High | Effort::Xhigh)) && route.stamp() == route.effort.map(Effort::arm)), "{routes:?}");
        assert_eq!(routes, draw(category), "the draw is seeded by the item and its launch count");
    }
    let drawn = draw("planning").into_iter().enumerate().find(|(_, route)| route.effort == Some(Effort::Xhigh)).unwrap();
    assert_eq!(drawn.1.stamp(), Some("effort-xhigh"));
    let mut history = History::default();
    let item = format!("north#{}", drawn.0 + 1);
    history.launched(&item, "first", &drawn.1);
    assert!(drawn.1.why.contains(&format!("seeded effort:{item}#0")) && route_for(&item, &facet("planning"), &history, &unread).unwrap().why.contains(&format!("seeded effort:{item}#1")));

    let full = |usd: f64| (15, 15, usd);
    let boxes = |(_, closed, usd): (usize, usize, f64)| vec![crate::router::settle::Outcome { usd, closed: true, kept: true }; closed];
    let trial = |high: (usize, usize, f64), xhigh: (usize, usize, f64)| Trial {
        runs: BTreeMap::from([(Effort::High, high.0), (Effort::Xhigh, xhigh.0)]),
        boxes: Some(BTreeMap::from([(Effort::High, boxes(high)), (Effort::Xhigh, boxes(xhigh))])),
    };
    let settled = |efforts| route_for("north#1", &facet("planning"), &History::default(), &BTreeMap::from([("planning".to_owned(), Evidence { efforts, ..Evidence::default() })])).unwrap();
    let cheaper_xhigh = settled(trial(full(12.0), full(9.0)));
    assert_eq!((cheaper_xhigh.effort, cheaper_xhigh.stamp()), (Some(Effort::Xhigh), None));
    assert_eq!(settled(trial(full(9.0), full(12.0))).effort, Some(Effort::High));
    assert!(settled(trial(full(12.0), (14, 15, 9.0))).effort_arm, "xhigh one run short keeps drawing");

    let draw = || (1..=4000).map(|n| context_arm(&format!("north#{n}"), "docs-policy", &History::default(), &Evidence::default()).0).collect::<Vec<_>>();
    let arms = draw();
    for window in CONTEXT_WINDOWS {
        let arm = crate::fleet::compaction::arm(window);
        let share = arms.iter().filter(|drawn| **drawn == arm).count();
        assert!((850..=1150).contains(&share), "{arm}: {share} of 4000");
    }
    assert_eq!(arms, draw(), "the draw is seeded by the item and its launch count");

    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None, builds: None }];
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
    let hand = |category: &str| {
        let brief = format!("Item: north#7\nCategory: {category}");
        start(&Ask { item: "north#7", brief: &brief, prompt: &format!("{brief}\n\nDo the box."), model: None, reason: None, worker: true, places: false }, "", &cfg, &History::default(), &unread, &|_| super::grounds::Grounds::default())
    };
    for (category, tier) in [("tooling", Tier::Sonnet), ("feature", Tier::Sonnet), ("bug-known-cause", Tier::Sonnet), ("docs-policy", Tier::Sonnet), ("debugging-unknown-cause", Tier::Opus), ("mechanical", Tier::Haiku)] {
        let picked = hand(category).unwrap().route;
        let dispatched = route_for("north#7", &facet(category), &History::default(), &unread).unwrap();
        assert_eq!((picked.tier, &picked.category), (tier, &dispatched.category));
        assert_eq!(dispatched.tier, tier, "{category}: a hand launch starts where dispatch does");
    }
    let refused = hand("bug").unwrap_err();
    assert!(refused.contains("`Category: bug` is no category") && refused.contains("bug-known-cause"), "{refused}");
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, grant: None }];
    let skipped = decide(&survey(vec![uncategorised("north", 637, "Category: bug\n- [ ] a box", &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert!(skipped.launches.is_empty() && skipped.skipped.iter().any(|(_, why)| why.contains(&refused)), "{:?}", skipped.skipped);
}

#[test]
fn a_hand_launched_worker_brief_with_no_arm_draws_the_context_arm_dispatch_would_and_launches_claude_with_its_auto_compact_window() {
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
    let account = Account { provider: Claude.provider(), label: "a".into(), home: scratch.path().join("home"), token_file: None, api: None };
    let window = |prompt: &str| {
        let request = LaunchRequest {
            verification: false,
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
            slots: Default::default(),
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
    assert_eq!((lead.prompt.matches("Arm: ").count(), window(&lead.prompt)), (0, None));
}
