use super::plan::Cloud;
use super::tests::{issue, survey};
use super::{decide, slots};

fn uncategorised(repo: &str, number: u64, body: &str, labels: &[&str]) -> super::issues::Issue {
    super::issues::Issue { body: body.into(), ..issue(repo, number, "", labels) }
}


#[test]
fn a_rolling_alarm_issue_is_ready_but_never_placed_because_its_body_declares_no_category_while_the_same_body_with_one_launches() {
    let rise = crate::tune::rises::Rise { tier: "sonnet".into(), category: "tooling".into(), before: (5.2, 3), after: (10.4, 3), boxes: Vec::new() };
    let body = crate::tune::rises::issue_body(&[rise]).unwrap();
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None, readings: Vec::new(), workers: None }];
    let alarm = decide(&survey(vec![uncategorised("north", 608, &body, &[])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()), "test");
    assert!(alarm.launches.is_empty(), "{:?}", alarm.skipped);
    assert_eq!(alarm.skipped, [(super::issues::IssueRef::parse("north#608").unwrap(), format!("ready, not placed: {}", super::facets::UNCATEGORISED))]);

    let headed = decide(&survey(vec![uncategorised("north", 608, &format!("Category: tooling\n{body}"), &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    assert_eq!(headed.launches.len(), 1, "the header alone is what staffed north#608: {:?}", headed.skipped);
}

#[test]
fn a_planner_or_reviewer_run_takes_opus_high_and_a_seeded_half_take_xhigh_stamped_as_its_arm_until_the_ledger_settles_the_cheaper() {
    use super::tier::{Effort, Evidence, History, Tier, Trial, route_for};
    use std::collections::BTreeMap;
    let facet = |category: &'static str| move |key: &str| if key == "Category" { category.to_owned() } else { super::facets::default(key).to_owned() };
    let unread = BTreeMap::new();
    let draw = |category: &'static str| (1..=1000).map(|n| route_for(&format!("north#{n}"), &facet(category), &History::default(), &unread)).collect::<Vec<_>>();
    for category in ["planning", "review"] {
        let routes = draw(category);
        let xhigh = routes.iter().filter(|route| route.effort == Some(Effort::Xhigh)).count();
        assert!((450..=550).contains(&xhigh), "{category}: {xhigh} of 1000 drew xhigh");
        assert!(routes.iter().all(|route| route.tier == Tier::Opus && matches!(route.effort, Some(Effort::High | Effort::Xhigh)) && route.stamp() == route.effort.map(Effort::arm)), "{routes:?}");
        assert_eq!(routes, draw(category), "the draw is seeded by the item and its launch count");
    }
    let drawn = draw("planning").into_iter().enumerate().find(|(_, route)| route.effort == Some(Effort::Xhigh)).unwrap();
    assert_eq!(drawn.1.effort_stamp(), Some("effort-xhigh"));
    let mut history = History::default();
    let item = format!("north#{}", drawn.0 + 1);
    history.launched(&item, "first", &drawn.1);
    assert!(drawn.1.why.contains(&format!("seeded {item}#0")) && route_for(&item, &facet("planning"), &history, &unread).why.contains(&format!("seeded {item}#1")));

    let full = |usd: f64| (15, 15, usd);
    let trial = |high: (usize, usize, f64), xhigh: (usize, usize, f64)| Trial {
        runs: BTreeMap::from([(Effort::High, high.0), (Effort::Xhigh, xhigh.0)]),
        closed: BTreeMap::from([(Effort::High, high.1), (Effort::Xhigh, xhigh.1)]),
        usd_per_kept: Some(BTreeMap::from([(Effort::High, high.2), (Effort::Xhigh, xhigh.2)])),
    };
    let settled = |efforts| route_for("north#1", &facet("planning"), &History::default(), &BTreeMap::from([("planning".to_owned(), Evidence { efforts, ..Evidence::default() })]));
    let cheaper_xhigh = settled(trial(full(12.0), full(9.0)));
    assert_eq!((cheaper_xhigh.effort, cheaper_xhigh.stamp()), (Some(Effort::Xhigh), None));
    assert_eq!(settled(trial(full(9.0), full(12.0))).effort, Some(Effort::High));
    assert!(settled(trial(full(12.0), (14, 15, 9.0))).effort_arm, "xhigh one run short keeps drawing");
}

#[test]
fn a_local_box_on_no_tier_or_effort_arm_draws_each_context_window_in_an_equal_seeded_share_and_is_stamped_with_it() {
    use super::tier::{CONTEXT_WINDOWS, History, context_arm};
    let draw = || (1..=4000).map(|n| context_arm(&format!("north#{n}"), &History::default())).collect::<Vec<_>>();
    let arms = draw();
    for window in CONTEXT_WINDOWS {
        let arm = crate::fleet::compaction::arm(window);
        let share = arms.iter().filter(|drawn| **drawn == arm).count();
        assert!((850..=1150).contains(&share), "{arm}: {share} of 4000");
    }
    assert_eq!(arms, draw(), "the draw is seeded by the item and its launch count");

    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None, builds: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None, readings: Vec::new(), workers: None }];
    let launched = decide(&survey(vec![uncategorised("north", 636, "Category: docs-policy\n- [ ] a box", &[])], Cloud::Closed(String::new()), hosts, accounts), "test");
    let [launch] = launched.launches.as_slice() else { panic!("{:?}", launched.skipped) };
    let stamp = format!("Arm: {}", context_arm("north#636", &History::default()));
    let lines: Vec<&str> = launch.brief.lines().collect();
    let tier = lines.iter().position(|line| line.starts_with("Tier: ")).unwrap();
    assert_eq!(lines[tier + 1], stamp, "{}", launch.brief);
    assert_eq!(crate::fleet::compaction::window(&launch.brief).map(crate::fleet::compaction::arm), Some(stamp.trim_start_matches("Arm: ").to_owned()));
}
