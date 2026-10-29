use super::plan::Cloud;
use super::record::Record;
use super::tests::{issue, survey};
use super::tier::{ARM_RUNS, Evidence, History, Tier};
use crate::ledger::closed::{ArmCell, Cell, ClosedReport};
use super::{alert, decide, effects, slots};
use std::collections::BTreeMap;

const RECOMMEND: &str = "tier claude:sonnet-high  cell [Category: tooling]  n 30
success threshold 80%  rule $ per success (no tier passes; lowest measured cost among sampled tiers)
  claude:sonnet-high 73% of 15  $0.21/success (priced n=15)
  claude:opus-medium 66% of 15  $0.60/success (priced n=15)
  claude-sonnet-5-5 medium 33% of 6  $0.09/success (priced n=5)
eta 32m (p75 of n=15, cell Category: tooling, tier claude:sonnet-high)";

fn placed(boxes: Vec<super::issues::Issue>, history: History, ledger: BTreeMap<String, Evidence>) -> Vec<super::plan::Launch> {
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let mut survey = survey(boxes, Cloud::Closed(String::new()), hosts, accounts);
    (survey.history, survey.ledger) = (history, ledger);
    decide(&survey, "test").launches
}

fn launched(boxes: Vec<super::issues::Issue>, history: History, ledger: BTreeMap<String, Evidence>) -> Vec<(String, Tier, bool, String)> {
    placed(boxes, history, ledger).into_iter().map(|launch| (launch.item.to_string(), launch.route.tier, launch.route.arm, launch.brief)).collect()
}

fn tiers(launches: &[(String, Tier, bool, String)]) -> Vec<(&str, &str)> {
    launches.iter().map(|(item, tier, _, _)| (item.as_str(), tier.name())).collect()
}

fn uncategorised(repo: &str, number: u64, body: &str, labels: &[&str]) -> super::issues::Issue {
    super::issues::Issue { body: body.into(), ..issue(repo, number, "", labels) }
}

fn haiku_try(item: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: "sc-3-d0900".into(), cloud: false, launched: true, detail: String::new(), tier: Some("worker-haiku".into()), category: Some("mechanical".into()), arm: false, handoff: None, hand: false }
}

#[test]
fn known_cause_bugs_and_judged_art_start_on_sonnet_mechanical_work_on_haiku_and_unknown_causes_on_opus() {
    let boxes = vec![
        issue("smashcraft", 1, "- [ ] a\nCategory: bug-known-cause", &[]),
        issue("smashcraft", 2, "- [ ] a\nCategory: feature\nSpec: judged\nSurface: assets\nVerify: visual", &[]),
        issue("smashcraft", 3, "- [ ] a\nCategory: mechanical", &[]),
        issue("smashcraft", 4, "- [ ] a\nCategory: debugging-unknown-cause", &[]),
        issue("smashcraft", 5, "- [ ] a\nCategory: anim", &[]),
        issue("smashcraft", 6, "- [ ] a\nCategory: netcode-determinism", &[]),
    ];
    let launches = launched(boxes, History::default(), BTreeMap::new());
    assert_eq!(
        tiers(&launches),
        [("smashcraft#1", "sonnet-high"), ("smashcraft#2", "sonnet-high"), ("smashcraft#3", "worker-haiku"), ("smashcraft#4", "worker"), ("smashcraft#5", "sonnet-high"), ("smashcraft#6", "worker")]
    );
    assert!(launches[0].2 && launches[0].3.contains("Arm: sonnet-high"), "a known-cause bug is an arm, Sonnet first");
    assert!(launches.iter().skip(1).all(|(_, tier, arm, brief)| !arm && !brief.contains(&format!("Arm: {}", tier.name()))), "only the armed categories run as arms");
    assert!(launches[2].3.contains("Tier: worker-haiku") && launches[2].3.contains("Monitor"), "the Haiku brief says to wait on runs with Monitor");
    assert!(!launches[0].3.contains("Monitor"));
}

#[test]
fn a_mechanical_box_haiku_missed_retries_on_sonnet_following_the_haiku_session() {
    let records = [haiku_try("smashcraft#3", 100.0)];
    let launches = launched(vec![issue("smashcraft", 3, "- [ ] a\nCategory: mechanical", &[])], History::new(&records), BTreeMap::new());
    assert_eq!(tiers(&launches), [("smashcraft#3", "sonnet-high")]);
    assert!(launches[0].3.contains("Follows: sc-3-d0900 (worker-haiku)"), "{}", launches[0].3);
    let old: Record = serde_json::from_str(r#"{"at":1.0,"item":"smashcraft#3","host":"gex45","account":"claude:b","name":"sc-3","launched":true,"detail":""}"#).unwrap();
    assert_eq!((old.tier, old.arm), (None, false), "records from before tiers still read");
    let never = Record { launched: false, ..haiku_try("smashcraft#3", 100.0) };
    assert_eq!(tiers(&launched(vec![issue("smashcraft", 3, "- [ ] a\nCategory: mechanical", &[])], History::new(&[never]), BTreeMap::new())), [("smashcraft#3", "worker-haiku")], "a launch that never started is no miss");
}

#[test]
fn tooling_and_features_alternate_recorded_arms_until_each_has_fifteen_runs_then_take_the_cheaper_arm() {
    let tooling = |number: u64| issue("wisp", number, "- [ ] a\nCategory: tooling", &[]);
    let launches = launched(vec![tooling(1), tooling(2), tooling(3), issue("wisp", 4, "- [ ] a", &[])], History::default(), BTreeMap::new());
    assert_eq!(tiers(&launches), [("wisp#1", "sonnet-high"), ("wisp#2", "worker"), ("wisp#3", "sonnet-high"), ("wisp#4", "sonnet-high")], "a feature box has its own arms");
    assert!(launches.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
    let last = Record { tier: Some("sonnet-high".into()), category: Some("tooling".into()), arm: true, name: "wisp-1-d0800".into(), item: "wisp#1".into(), ..haiku_try("wisp#1", 50.0) };
    assert_eq!(tiers(&launched(vec![tooling(5)], History::new(&[last]), BTreeMap::new())), [("wisp#5", "worker")], "the next run takes the arm after the recorded one");
    let evidence = Evidence::parse(RECOMMEND, "tooling");
    assert_eq!((evidence.tiers["claude:sonnet-high"], evidence.tiers["claude:opus-medium"], evidence.tiers["claude-sonnet-5-5 medium"]), ((73.0, ARM_RUNS), (66.0, ARM_RUNS), (33.0, 6)));
    let decided = launched(vec![tooling(6), tooling(7)], History::default(), BTreeMap::from([("tooling".to_owned(), evidence.clone().priced(&closed(&[("sonnet", 5, 3.0), ("opus-medium", 5, 9.0)]), "tooling"))]));
    assert_eq!(tiers(&decided), [("wisp#6", "sonnet-high"), ("wisp#7", "sonnet-high")], "both arms at 15 runs: the cheaper arm, every time");
    assert!(decided.iter().all(|(_, tier, arm, brief)| !arm && !brief.contains(&format!("Arm: {}", tier.name()))));
    let short = Evidence { tiers: BTreeMap::from([("claude:sonnet-high".to_owned(), (73.0, 15)), ("claude:opus-medium".to_owned(), (66.0, 14))]), ..evidence.clone() };
    let alternating = launched(vec![tooling(8), tooling(9)], History::default(), BTreeMap::from([("tooling".to_owned(), short.priced(&closed(&[("sonnet", 5, 3.0), ("opus-medium", 5, 1.0)]), "tooling"))]));
    assert_eq!(tiers(&alternating), [("wisp#8", "sonnet-high"), ("wisp#9", "worker")]);
    assert!(alternating.iter().all(|(_, _, _, brief)| brief.contains("sonnet-high 73% of 15, $3.00 per closed box") && brief.contains("worker 66% of 14, $1.00 per closed box")), "an arm launch reason shows both arms' cost per closed box: {alternating:?}");
    assert_eq!(Evidence::parse(&RECOMMEND.replace("[Category: tooling]", "[all runs]"), "tooling"), Evidence::default(), "a coarser cell is not this category's evidence");
    assert_eq!(tiers(&launched(vec![tooling(10), tooling(11)], History::default(), BTreeMap::from([("tooling".to_owned(), evidence)]))), [("wisp#10", "sonnet-high"), ("wisp#11", "worker")], "arms with 15 runs and no closed-box report keep alternating");
}

fn closed(cells: &[(&str, usize, f64)]) -> ClosedReport {
    let other = Cell::priced("opus-medium", "feature", 1, 0.01);
    let cells: Vec<Cell> = cells.iter().map(|(tier, closed, per)| Cell::priced(tier, "tooling", *closed, per * *closed as f64)).chain([other]).collect();
    ClosedReport { closed: cells.iter().map(|cell| cell.closed).sum(), cells, arms: Vec::new(), spend_usd: 0.0, leads_usd: 0.0 }
}

fn armed(sonnet: (f64, usize), opus: (f64, usize), report: &ClosedReport) -> BTreeMap<String, Evidence> {
    let tiers = BTreeMap::from([("claude:sonnet-high".to_owned(), sonnet), ("claude:opus-medium".to_owned(), opus)]);
    BTreeMap::from([("tooling".to_owned(), Evidence { tiers, usd_per_closed: None }.priced(report, "tooling"))])
}

#[test]
fn the_arm_switch_takes_the_lower_cost_per_closed_box_not_the_higher_done_rate() {
    let tooling = |number: u64| issue("wisp", number, "- [ ] a\nCategory: tooling", &[]);
    let pick = |ledger: BTreeMap<String, Evidence>| {
        let launch = placed(vec![tooling(1)], History::default(), ledger).remove(0);
        (launch.route.tier.name(), launch.route.arm, launch.route.why)
    };
    let report = closed(&[("sonnet", 9, 3.0), ("opus-medium", 10, 9.0)]);
    let (tier, arm, why) = pick(armed((60.0, 15), (70.0, 15), &report));
    assert_eq!((tier, arm), ("sonnet-high", false), "Sonnet closes 60% at $3 per closed box and Opus 70% at $9: Sonnet");
    assert!(why.contains("sonnet-high 60% of 15, $3.00 per closed box") && why.contains("worker 70% of 15, $9.00 per closed box"), "{why}");
    let flipped = closed(&[("sonnet", 9, 9.0), ("opus-medium", 10, 3.0)]);
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &flipped)).0, "worker", "Opus when it is the cheaper arm");
    let merged = closed(&[("sonnet", 1, 2.0), ("claude-sonnet-5-5 medium", 3, 4.0), ("opus-medium", 1, 3.4)]);
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &merged)).0, "worker", "a tier's cells merge by closed boxes: Sonnet $3.50 against Opus $3.40");
    for lone in [closed(&[("sonnet", 4, 3.0)]), closed(&[("opus-medium", 4, 9.0)]), closed(&[])] {
        let (_, arm, why) = pick(armed((60.0, 15), (70.0, 15), &lone));
        assert!(arm, "an arm with no closed box in the window keeps the arms alternating: {why}");
    }
    let tie = closed(&[("sonnet", 4, 3.0), ("opus-medium", 4, 3.0)]);
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &tie)).0, "sonnet-high", "Sonnet on a tie");
}

#[test]
fn a_launch_runs_its_tiers_model_and_effort_and_an_unnamed_tier_fails_it() {
    let launches = {
        let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
        let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
        let survey = survey(vec![issue("smashcraft", 3, "- [ ] a\nCategory: mechanical", &[]), issue("smashcraft", 1, "- [ ] a\nCategory: tooling", &[])], Cloud::Closed(String::new()), hosts, accounts);
        let outcome = decide(&survey, "test");
        outcome.launches.iter().map(|launch| effects::command(launch, "sc-1", &survey, "/home/tom", "north").unwrap().1.join(" ")).collect::<Vec<_>>()
    };
    assert!(launches[0].contains("--model claude-sonnet-5-5 --effort high"), "{}", launches[0]);
    assert!(launches[1].contains("--model claude-haiku-5-5 --effort high"), "{}", launches[1]);
    let bare = survey(Vec::new(), Cloud::Closed(String::new()), Vec::new(), Vec::new());
    let bare = super::survey::Survey { cfg: "[hosts.gex45]\n".parse().unwrap(), ..bare };
    let launch = super::plan::Launch { item: issue("smashcraft", 1, "", &[]).reference(), host: "gex45".into(), account: "claude:b".into(), brief: String::new(), route: route(Tier::Opus), cloud: false, local: None, pin: None, handoff: None, burn: None };
    assert!(effects::command(&launch, "sc-1", &bare, "/home/tom", "north").unwrap_err().contains("claude:opus-medium"), "a tier orchestration does not name fails the launch, never falls back to the default model");
}

pub(super) fn route(tier: Tier) -> super::tier::Route {
    super::tier::Route { tier, category: "feature".into(), arm: false, follows: None, why: String::new() }
}

#[test]
fn fleet_launches_alternate_the_handoff_arms_per_tier_first_in_the_brief_until_each_tier_holds_thirty() {
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let boxes = |category: &str| (1..=3).map(|number| issue("smashcraft", number, &format!("- [ ] a\nCategory: {category}"), &[])).collect::<Vec<_>>();
    let arms = |survey: &super::survey::Survey| -> Vec<(Option<String>, Option<String>)> {
        decide(survey, "test").launches.into_iter().map(|launch| (launch.handoff, launch.brief.lines().find_map(|line| line.strip_prefix("Arm: ")).map(str::to_owned))).collect()
    };
    let (on, off) = (Some("handoff-150k".to_owned()), Some("no-handoff".to_owned()));
    let fresh = survey(boxes("debugging-unknown-cause"), Cloud::Closed(String::new()), hosts.clone(), accounts.clone());
    assert_eq!(arms(&fresh), [(on.clone(), on.clone()), (off.clone(), off.clone()), (on.clone(), on.clone())], "the handoff arm is the brief's first Arm line, which the ledger records");
    let record = |handoff: &Option<String>| Record { tier: Some("worker".into()), handoff: handoff.clone(), ..haiku_try("smashcraft#9", 1.0) };
    let held = |on_runs: usize, off_runs: usize| super::handoff::Arms::new(&[vec![record(&on); on_runs], vec![record(&off); off_runs]].concat());
    let lagging = super::survey::Survey { handoffs: held(30, 29), ..survey(boxes("debugging-unknown-cause"), Cloud::Closed(String::new()), hosts.clone(), accounts.clone()) };
    assert_eq!(arms(&lagging), [(off.clone(), off.clone()), (None, None), (None, None)], "only the arm under 30 runs on that tier still launches");
    let sonnet = super::survey::Survey { handoffs: held(30, 30), ..survey(boxes("bug-known-cause"), Cloud::Closed(String::new()), hosts, accounts) };
    assert_eq!(arms(&sonnet)[0], (on.clone(), on), "each tier fills its own arms");
}

#[test]
fn both_handoff_arms_at_thirty_runs_go_to_the_lower_cost_per_closed_box_and_the_north_57_recommendation_is_emitted() {
    let (on, off) = ("handoff-150k", "no-handoff");
    let record = |handoff: &str| Record { tier: Some("worker".into()), handoff: Some(handoff.into()), ..haiku_try("smashcraft#9", 1.0) };
    let held = |on_runs: usize, off_runs: usize| super::handoff::Arms::new(&[vec![record(on); on_runs], vec![record(off); off_runs]].concat());
    let costed = |on_usd: f64, off_usd: f64| ClosedReport { arms: vec![ArmCell { tier: "opus-medium".into(), arm: on.into(), closed: 5, usd: on_usd * 5.0 }, ArmCell { tier: "opus-medium".into(), arm: off.into(), closed: 5, usd: off_usd * 5.0 }], ..closed(&[]) };
    let mut cheaper_without = held(30, 30).priced(&costed(10.0, 6.0));
    assert_eq!(cheaper_without.pick(Tier::Opus).as_deref(), Some(off));
    assert_eq!(cheaper_without.pick(Tier::Opus).as_deref(), Some(off), "a settled tier keeps launching its winner");
    assert!(cheaper_without.recommendations()[0].contains("no-handoff wins at $6.00") && cheaper_without.recommendations()[0].contains("handoff-150k at $10.00"));
    let mut cheaper_with = held(30, 30).priced(&costed(4.0, 6.0));
    assert_eq!(cheaper_with.pick(Tier::Opus).as_deref(), Some(on));
    assert!(cheaper_with.recommendations()[0].contains("handoff-150k wins at $4.00") && cheaper_with.recommendations()[0].ends_with("keep the worker handoff point for this tier"));
    let mut tied = held(30, 30).priced(&costed(6.0, 6.0));
    assert_eq!(tied.pick(Tier::Opus).as_deref(), Some(on), "a tie keeps the incumbent handoff arm");
    let mut lagging = held(30, 29).priced(&costed(10.0, 6.0));
    assert!(lagging.recommendations().is_empty());
    assert_eq!(lagging.pick(Tier::Opus).as_deref(), Some(off), "a tier short of 30 runs on an arm keeps filling it");
    let mut unpriced = held(30, 30).priced(&closed(&[]));
    assert!(unpriced.recommendations().is_empty());
    assert_eq!(unpriced.pick(Tier::Opus), None, "30 runs each without a closed box on both arms holds the arms as they were");
    assert_eq!(held(30, 30).priced(&costed(10.0, 6.0)).pick(Tier::Sonnet).as_deref(), Some(on), "another tier's runs are its own");
}

#[test]
fn a_box_without_a_category_line_takes_one_from_its_labels_and_one_nothing_categorises_is_refused_with_the_reason() {
    let boxes = vec![
        uncategorised("north", 1, "- [ ] a", &["enhancement", "priority:now"]),
        uncategorised("north", 2, "- [ ] a", &["bug"]),
        uncategorised("north", 3, "- [ ] a", &["priority:next", "category:mechanical"]),
        uncategorised("north", 4, "- [ ] a\n- Category: bug-known-cause. Spec: measured. Scope: padbridge.", &[]),
    ];
    let mut launches = launched(boxes, History::default(), BTreeMap::new());
    launches.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(tiers(&launches), [("north#1", "sonnet-high"), ("north#2", "worker"), ("north#3", "worker-haiku"), ("north#4", "sonnet-high")]);
    let headers: Vec<bool> = launches.iter().zip(["feature", "debugging-unknown-cause", "mechanical", "bug-known-cause"]).map(|((_, _, _, brief), category)| brief.contains(&format!("\nCategory: {category}\n"))).collect();
    assert_eq!(headers, [true; 4], "the brief names the derived category");

    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let blind = survey(vec![uncategorised("north", 5, "- [ ] a", &["priority:now"])], Cloud::Closed(String::new()), hosts, accounts);
    let outcome = decide(&blind, "test");
    assert!(outcome.launches.is_empty(), "an item nothing categorises does not launch");
    assert_eq!(outcome.skipped, [(super::issues::IssueRef::parse("north#5").unwrap(), format!("ready, not placed: {}", super::facets::UNCATEGORISED))]);
    assert!(alert(&outcome, &[], "nexus").is_some_and(|line| line.contains("north#5 ready, not placed: no category")), "the refusal reaches the alert");
}

#[test]
fn a_feature_box_launches_on_worker_after_a_recorded_sonnet_feature_launch_until_both_arms_hold_fifteen_runs() {
    let feature = |number: u64| issue("smashcraft", number, "- [ ] a\nCategory: feature", &[]);
    let last = Record { tier: Some("sonnet-high".into()), category: Some("feature".into()), arm: true, name: "sc-1-d0800".into(), item: "smashcraft#1".into(), ..haiku_try("smashcraft#1", 50.0) };
    let launches = launched(vec![feature(2), feature(3)], History::new(&[last]), BTreeMap::new());
    assert_eq!(tiers(&launches), [("smashcraft#2", "worker"), ("smashcraft#3", "sonnet-high")]);
    assert!(launches.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
}
