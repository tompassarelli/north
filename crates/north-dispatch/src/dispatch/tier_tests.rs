use super::plan::Cloud;
use super::record::Record;
use super::tests::{issue, survey};
use crate::router::policy::ARM_RUNS;
use super::tier::{Evidence, History, Tier};
use crate::ledger::closed::{ArmBox, Cell, ClosedReport};
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
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: "sc-3-d0900".into(), cloud: false, launched: true, detail: String::new(), tier: Some("worker-haiku".into()), category: Some("mechanical".into()), arm: false, handoff: None, hand: false, effort_arm: None }
}

#[test]
fn known_cause_bugs_and_judged_art_start_on_sonnet_mechanical_work_on_haiku_and_unknown_causes_on_opus() {
    let boxes = vec![
        issue("makgora", 1, "- [ ] a\nCategory: bug-known-cause\nClass: a stale frame", &[]),
        issue("makgora", 2, "- [ ] a\nCategory: feature\nSpec: judged\nSurface: assets\nVerify: visual", &[]),
        issue("makgora", 3, "- [ ] a\nCategory: mechanical", &[]),
        issue("makgora", 4, "- [ ] a\nCategory: debugging-unknown-cause\nClass: a lost input", &[]),
        issue("makgora", 5, "- [ ] a\nCategory: anim", &[]),
        issue("makgora", 6, "- [ ] a\nCategory: netcode-determinism", &[]),
    ];
    let launches = launched(boxes, History::default(), BTreeMap::new());
    assert_eq!(
        tiers(&launches),
        [("makgora#1", "sonnet-high"), ("makgora#2", "sonnet-high"), ("makgora#3", "worker-haiku"), ("makgora#4", "worker"), ("makgora#5", "sonnet-high"), ("makgora#6", "worker")]
    );
    assert!(launches[0].2 && launches[0].3.contains("Arm: sonnet-high"), "a known-cause bug is an arm, Sonnet first");
    assert!(launches.iter().skip(1).all(|(_, tier, arm, brief)| !arm && !brief.contains(&format!("Arm: {}", tier.name()))), "only the armed categories run as arms");
    assert!(launches[2].3.contains("Tier: worker-haiku") && launches[2].3.contains("Monitor"), "the Haiku brief says to wait on runs with Monitor");
    assert!(!launches[0].3.contains("Monitor"));
}

#[test]
fn a_mechanical_box_haiku_missed_retries_on_sonnet_following_the_haiku_session() {
    let records = [haiku_try("makgora#3", 100.0)];
    let launches = launched(vec![issue("makgora", 3, "- [ ] a\nCategory: mechanical", &[])], History::new(&records), BTreeMap::new());
    assert_eq!(tiers(&launches), [("makgora#3", "sonnet-high")]);
    assert!(launches[0].3.contains("Follows: sc-3-d0900 (worker-haiku)"), "{}", launches[0].3);
    let old: Record = serde_json::from_str(r#"{"at":1.0,"item":"makgora#3","host":"gex45","account":"claude:b","name":"sc-3","launched":true,"detail":""}"#).unwrap();
    assert_eq!((old.tier, old.arm), (None, false), "records from before tiers still read");
    let never = Record { launched: false, ..haiku_try("makgora#3", 100.0) };
    assert_eq!(tiers(&launched(vec![issue("makgora", 3, "- [ ] a\nCategory: mechanical", &[])], History::new(&[never]), BTreeMap::new())), [("makgora#3", "worker-haiku")], "a launch that never started is no miss");
}

#[test]
fn tooling_and_features_alternate_recorded_arms_until_each_has_fifteen_runs_then_take_the_cheaper_arm() {
    let tooling = |number: u64| issue("wisp", number, "- [ ] a\nCategory: tooling", &[]);
    let launches = launched(vec![tooling(1), tooling(2), tooling(3), issue("wisp", 4, "- [ ] a", &[])], History::default(), BTreeMap::new());
    assert_eq!(tiers(&launches), [("wisp#1", "sonnet-high"), ("wisp#2", "worker"), ("wisp#3", "sonnet-high"), ("wisp#4", "sonnet-high")], "a feature box has its own arms");
    assert!(launches.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
    let last = Record { tier: Some("sonnet-high".into()), category: Some("tooling".into()), arm: true, name: "wisp-1-d0800".into(), item: "wisp#1".into(), ..haiku_try("wisp#1", 50.0) };
    assert_eq!(tiers(&launched(vec![tooling(5)], History::new(&[last]), BTreeMap::new())), [("wisp#5", "worker")], "the next run takes the arm after the recorded one");
    let evidence = Evidence::parse(RECOMMEND, "tooling").counted(&stamped(15, 15));
    assert_eq!((evidence.tiers["claude:sonnet-high"], evidence.tiers["claude:opus-medium"], evidence.tiers["claude-sonnet-5-5 medium"]), ((73.0, ARM_RUNS), (66.0, ARM_RUNS), (33.0, 6)));
    let decided = launched(vec![tooling(6), tooling(7)], History::default(), BTreeMap::from([("tooling".to_owned(), evidence.clone().priced(&closed(&[("sonnet-high", 15, 3.0), ("worker", 15, 9.0)]), "tooling"))]));
    assert_eq!(tiers(&decided), [("wisp#6", "sonnet-high"), ("wisp#7", "sonnet-high")], "both arms at 15 runs: the cheaper arm, every time");
    assert!(decided.iter().all(|(_, tier, arm, brief)| !arm && !brief.contains(&format!("Arm: {}", tier.name()))));
    let short = evidence.clone().counted(&stamped(15, 14));
    let alternating = launched(vec![tooling(8), tooling(9)], History::default(), BTreeMap::from([("tooling".to_owned(), short.priced(&closed(&[("sonnet-high", 15, 3.0), ("worker", 15, 1.0)]), "tooling"))]));
    assert_eq!(tiers(&alternating), [("wisp#8", "sonnet-high"), ("wisp#9", "worker")]);
    assert!(alternating.iter().all(|(_, _, _, brief)| brief.contains("sonnet-high 73% of 15, 15 of 15 arm runs, 15 of 15 closed arm boxes, $3.00 per closed box") && brief.contains("worker 66% of 15, 14 of 15 arm runs, 15 of 15 closed arm boxes, $1.00 per closed box")), "an arm launch reason shows both arms' cost per closed box: {alternating:?}");
    assert_eq!(Evidence::parse(&RECOMMEND.replace("[Category: tooling]", "[all runs]"), "tooling"), Evidence::default(), "a coarser cell is not this category's evidence");
    assert_eq!(tiers(&launched(vec![tooling(10), tooling(11)], History::default(), BTreeMap::from([("tooling".to_owned(), evidence)]))), [("wisp#10", "sonnet-high"), ("wisp#11", "worker")], "arms with 15 runs and no closed-box report keep alternating");
}

fn closed(arms: &[(&str, usize, f64)]) -> ClosedReport {
    let arm_box = |arm: &str, category: &str, number: usize, usd: f64| ArmBox { item: format!("wisp#{arm}-{number}"), tier: String::new(), arm: arm.into(), usd, category: category.into(), closed: true, reworked: false };
    let boxes = arms.iter().flat_map(|(arm, closed, usd)| (0..*closed).map(move |number| arm_box(arm, "tooling", number, *usd)));
    ClosedReport { skipped: Vec::new(), closed: 0, cells: Vec::new(), arms: boxes.chain([arm_box("worker", "feature", 0, 0.01)]).collect(), spend_usd: 0.0, leads_usd: 0.0, ..ClosedReport::default() }
}

fn armed(sonnet: (f64, usize), opus: (f64, usize), report: &ClosedReport) -> BTreeMap<String, Evidence> {
    let tiers = BTreeMap::from([("claude:sonnet-high".to_owned(), sonnet), ("claude:opus-medium".to_owned(), opus)]);
    BTreeMap::from([("tooling".to_owned(), Evidence { tiers, arm_runs: BTreeMap::from([(Tier::Sonnet, sonnet.1), (Tier::Opus, opus.1)]), ..Evidence::default() }.priced(report, "tooling"))])
}

fn stamped(sonnet: usize, opus: usize) -> BTreeMap<String, usize> {
    BTreeMap::from([("sonnet-high".to_owned(), sonnet), ("worker".to_owned(), opus)])
}

#[test]
fn a_tier_with_many_unstamped_runs_still_alternates_until_each_arm_holds_fifteen_stamped_runs() {
    let report = closed(&[("sonnet-high", 15, 3.0), ("worker", 15, 9.0)]);
    let feature = |counted: BTreeMap<String, usize>| {
        let all = Evidence::parse(&RECOMMEND.replace("tooling", "feature").replace("73% of 15", "42% of 80").replace("66% of 15", "37% of 154"), "feature");
        BTreeMap::from([("feature".to_owned(), all.counted(&counted).priced(&report, "feature"))])
    };
    let issue = |number: u64| issue("wisp", number, "- [ ] a\nCategory: feature", &[]);
    let alternating = launched(vec![issue(1), issue(2)], History::default(), feature(stamped(16, 11)));
    assert_eq!(tiers(&alternating), [("wisp#1", "sonnet-high"), ("wisp#2", "worker")], "80 and 154 runs overall, but the Opus arm holds 11 stamped runs");
    assert!(alternating.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
    assert!(alternating[0].3.contains("11 of 15 arm runs"), "{}", alternating[0].3);
    assert!(launched(vec![issue(3)], History::default(), feature(BTreeMap::new())).iter().all(|(_, _, arm, _)| *arm), "no stamped run counted: both arms still run");
}

#[test]
fn the_arm_switch_takes_the_lower_cost_per_closed_box_not_the_higher_done_rate() {
    let tooling = |number: u64| issue("wisp", number, "- [ ] a\nCategory: tooling", &[]);
    let pick = |ledger: BTreeMap<String, Evidence>| {
        let launch = placed(vec![tooling(1)], History::default(), ledger).remove(0);
        (launch.route.tier.name(), launch.route.arm, launch.route.why)
    };
    let report = closed(&[("sonnet-high", 15, 3.0), ("worker", 15, 9.0)]);
    let (tier, arm, why) = pick(armed((60.0, 15), (70.0, 15), &report));
    assert_eq!((tier, arm), ("sonnet-high", false), "Sonnet closes 60% at $3 per closed box and Opus 70% at $9: Sonnet");
    assert!(why.contains("sonnet-high 60% of 15, 15 of 15 arm runs, 15 of 15 closed arm boxes, $3.00 per closed box") && why.contains("worker 70% of 15, 15 of 15 arm runs, 15 of 15 closed arm boxes, $9.00 per closed box"), "{why}");
    let flipped = closed(&[("sonnet-high", 15, 9.0), ("worker", 15, 3.0)]);
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &flipped)).0, "worker", "Opus when it is the cheaper arm");
    let (_, arm, why) = pick(armed((60.0, 15), (70.0, 15), &closed(&[("sonnet-high", 15, 3.0), ("worker", 14, 9.0)])));
    assert!(arm && why.contains("14 of 15 closed arm boxes"), "15 runs on each arm but 14 closed Opus arm boxes keep the arms alternating: {why}");
    let mut unstamped = closed(&[("sonnet-high", 15, 9.0), ("worker", 15, 3.0)]);
    unstamped.cells.push(Cell::priced("sonnet", "tooling", 40, 40.0));
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &unstamped)).0, "worker", "a tier's unstamped boxes, here 40 Sonnet boxes at $1, do not price its arm");
    for lone in [closed(&[("sonnet-high", 15, 3.0)]), closed(&[("worker", 15, 9.0)]), closed(&[])] {
        let (_, arm, why) = pick(armed((60.0, 15), (70.0, 15), &lone));
        assert!(arm, "an arm with no closed box in the window keeps the arms alternating: {why}");
    }
    let tie = closed(&[("sonnet-high", 15, 3.0), ("worker", 15, 3.0)]);
    assert_eq!(pick(armed((60.0, 15), (70.0, 15), &tie)).0, "sonnet-high", "Sonnet on a tie");
}

#[test]
fn a_launch_runs_its_tiers_model_and_effort_and_an_unnamed_tier_fails_it() {
    let launches = {
        let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
        let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
        let survey = survey(vec![issue("makgora", 3, "- [ ] a\nCategory: mechanical", &[]), issue("makgora", 1, "- [ ] a\nCategory: tooling", &[])], Cloud::Closed(String::new()), hosts, accounts);
        let outcome = decide(&survey, "test");
        outcome.launches.iter().map(|launch| effects::command(launch, "sc-1", &survey, "/home/tom", "north").unwrap().1.join(" ")).collect::<Vec<_>>()
    };
    assert!(launches[0].contains("--model claude-sonnet-5-5 --effort high"), "{}", launches[0]);
    assert!(launches[1].contains("--model claude-haiku-5-5 --effort high"), "{}", launches[1]);
    let bare = survey(Vec::new(), Cloud::Closed(String::new()), Vec::new(), Vec::new());
    let bare = super::survey::Survey { cfg: "[hosts.gex45]\n".parse().unwrap(), ..bare };
    let launch = super::plan::Launch { item: issue("makgora", 1, "", &[]).reference(), host: "gex45".into(), account: "claude:b".into(), brief: String::new(), route: route(Tier::Opus), cloud: false, local: None, pin: None, handoff: None, burn: None };
    assert!(effects::command(&launch, "sc-1", &bare, "/home/tom", "north").unwrap_err().contains("claude:opus-medium"), "a tier orchestration does not name fails the launch, never falls back to the default model");
}

pub(super) fn route(tier: Tier) -> super::tier::Route {
    super::tier::Route { tier, category: "feature".into(), arm: false, effort: None, effort_arm: false, follows: None, why: String::new() }
}

#[test]
fn fleet_launches_alternate_the_handoff_arms_per_tier_first_in_the_brief_until_each_tier_holds_thirty() {
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let boxes = |category: &str| (1..=3).map(|number| issue("makgora", number, &format!("- [ ] a\nCategory: {category}\nClass: a lost input"), &[])).collect::<Vec<_>>();
    let arms = |survey: &super::survey::Survey| -> Vec<(Option<String>, Option<String>)> {
        decide(survey, "test").launches.into_iter().map(|launch| (launch.handoff, launch.brief.lines().find_map(|line| line.strip_prefix("Arm: ")).map(str::to_owned))).collect()
    };
    let (on, off) = (Some("handoff-150k".to_owned()), Some("no-handoff".to_owned()));
    let fresh = survey(boxes("debugging-unknown-cause"), Cloud::Closed(String::new()), hosts.clone(), accounts.clone());
    assert_eq!(arms(&fresh), [(on.clone(), on.clone()), (off.clone(), off.clone()), (on.clone(), on.clone())], "the handoff arm is the brief's first Arm line, which the ledger records");
    let record = |handoff: &Option<String>| Record { tier: Some("worker".into()), handoff: handoff.clone(), ..haiku_try("makgora#9", 1.0) };
    let held = |on_runs: usize, off_runs: usize| super::handoff::Arms::new(&[vec![record(&on); on_runs], vec![record(&off); off_runs]].concat());
    let lagging = super::survey::Survey { handoffs: held(30, 28), ..survey(boxes("debugging-unknown-cause"), Cloud::Closed(String::new()), hosts.clone(), accounts.clone()) };
    assert_eq!(arms(&lagging), [(off.clone(), off.clone()), (off.clone(), off.clone()), (on.clone(), on.clone())], "the arm under 30 runs on that tier fills first, then unpriced arms keep alternating");
    let sonnet = super::survey::Survey { handoffs: held(30, 30), ..survey(boxes("bug-known-cause"), Cloud::Closed(String::new()), hosts, accounts) };
    assert_eq!(arms(&sonnet)[0], (None, Some("sonnet-high".to_owned())), "a box on a tier arm carries that arm alone, since the ledger records only the first Arm line");
    let other = super::survey::Survey { handoffs: held(30, 30), ..survey(boxes("anim"), Cloud::Closed(String::new()), vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }], vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }]) };
    assert_eq!(arms(&other)[0], (on.clone(), on), "a Sonnet box on no tier arm fills the Sonnet tier's handoff arms");
}

#[test]
fn both_handoff_arms_at_thirty_runs_go_to_the_lower_cost_per_closed_box_and_the_north_57_recommendation_is_emitted() {
    let (on, off) = ("handoff-150k", "no-handoff");
    let record = |handoff: &str| Record { tier: Some("worker".into()), handoff: Some(handoff.into()), ..haiku_try("makgora#9", 1.0) };
    let held = |on_runs: usize, off_runs: usize| super::handoff::Arms::new(&[vec![record(on); on_runs], vec![record(off); off_runs]].concat());
    let boxes = |arm: &'static str, usd: f64| (0..5).map(move |number| ArmBox { item: format!("north#{arm}-{number}"), tier: "opus-medium".into(), arm: arm.into(), usd, category: "feature".into(), closed: true, reworked: false });
    let costed = |on_usd: f64, off_usd: f64| boxes(on, on_usd).chain(boxes(off, off_usd)).collect::<Vec<_>>();
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
    let mut unpriced = held(30, 30).priced(&[]);
    assert!(unpriced.recommendations().is_empty());
    assert_eq!([unpriced.pick(Tier::Opus), unpriced.pick(Tier::Opus)], [Some(on.to_owned()), Some(off.to_owned())], "30 runs each without a closed box on both arms keeps alternating them until both are priced");
    let mut seen_twice = held(30, 30).priced(&[costed(10.0, 6.0), boxes(on, 1.0).collect()].concat());
    assert_eq!(seen_twice.pick(Tier::Opus).as_deref(), Some(off), "a box two hosts' ledgers both hold is priced once, from the first");
    assert_eq!(held(30, 30).priced(&costed(10.0, 6.0)).pick(Tier::Sonnet).as_deref(), Some(on), "another tier's runs are its own");
}

#[test]
fn a_cheaper_handoff_arm_that_closes_more_than_ten_points_fewer_boxes_loses_to_the_costlier_arm_that_closes_more() {
    let (on, off) = ("handoff-150k", "no-handoff");
    let record = |handoff: &str| Record { tier: Some("worker".into()), handoff: Some(handoff.into()), ..haiku_try("makgora#9", 1.0) };
    let held = || super::handoff::Arms::new(&[vec![record(on); 30], vec![record(off); 30]].concat());
    let closed = |arm: &'static str, count: usize, usd: f64| (0..count).map(move |number| ArmBox { item: format!("north#{arm}-{number}"), tier: "opus-medium".into(), arm: arm.into(), usd, category: "feature".into(), closed: true, reworked: false });
    let priced = |on_closed: usize, off_closed: usize, on_usd: f64, off_usd: f64| held().priced(&closed(on, on_closed, on_usd).chain(closed(off, off_closed, off_usd)).collect::<Vec<_>>());

    let mut cheap_but_failing = priced(6, 18, 4.0, 6.0);
    assert!(cheap_but_failing.recommendations()[0].contains("no-handoff wins at $6.00 per closed box and 60% Done against handoff-150k at $4.00 and 20%"));
    assert_eq!(cheap_but_failing.pick(Tier::Opus).as_deref(), Some(off), "20% Done at $4.00 loses to 60% Done at $6.00");

    let mut cheap_and_close = priced(15, 18, 4.0, 6.0);
    assert!(cheap_and_close.recommendations()[0].contains("handoff-150k wins at $4.00 per closed box and 50% Done against no-handoff at $6.00 and 60%"));
    assert_eq!(cheap_and_close.pick(Tier::Opus).as_deref(), Some(on), "50% Done at $4.00 is within ten points of 60% Done at $6.00");

    assert_eq!(priced(18, 6, 8.0, 6.0).pick(Tier::Opus).as_deref(), Some(off), "a costlier handoff arm loses whatever its Done rate");
}

#[test]
fn a_box_without_a_category_line_takes_one_from_its_labels_and_one_nothing_categorises_is_refused_with_the_reason() {
    let boxes = vec![
        uncategorised("north", 1, "- [ ] a", &["enhancement", "priority:now"]),
        uncategorised("north", 2, "- [ ] a\n- Class: a landing called done without checking its live effect", &["bug"]),
        uncategorised("north", 3, "- [ ] a", &["priority:next", "category:mechanical"]),
        uncategorised("north", 4, "- [ ] a\n- Category: bug-known-cause. Spec: measured. Scope: padbridge.\n- Class: a dropped pad frame", &[]),
    ];
    let mut launches = launched(boxes, History::default(), BTreeMap::new());
    launches.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(tiers(&launches), [("north#1", "sonnet-high"), ("north#2", "worker"), ("north#3", "worker-haiku"), ("north#4", "sonnet-high")]);
    let headers: Vec<bool> = launches.iter().zip(["feature", "debugging-unknown-cause", "mechanical", "bug-known-cause"]).map(|((_, _, _, brief), category)| brief.contains(&format!("\nCategory: {category}\n"))).collect();
    assert_eq!(headers, [true; 4], "the brief names the derived category");

    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let unclassed = decide(&survey(vec![uncategorised("north", 6, "- [ ] a", &["bug"])], Cloud::Closed(String::new()), hosts.clone(), accounts.clone()), "test");
    assert!(unclassed.launches.is_empty(), "a bug box naming no class of failure does not launch");
    assert!(unclassed.skipped[0].1.starts_with("ready, not placed: Category debugging-unknown-cause has no Class: line"), "{:?}", unclassed.skipped);
    let blind = survey(vec![uncategorised("north", 5, "- [ ] a", &["priority:now"])], Cloud::Closed(String::new()), hosts, accounts);
    let outcome = decide(&blind, "test");
    assert!(outcome.launches.is_empty(), "an item nothing categorises does not launch");
    assert_eq!(outcome.skipped, [(super::issues::IssueRef::parse("north#5").unwrap(), format!("ready, not placed: {}", super::facets::UNCATEGORISED))]);
    assert!(alert(&outcome, &[], "nexus").is_some_and(|line| line.contains("north#5 ready, not placed: no category")), "the refusal reaches the alert");
}

#[test]
fn a_feature_box_launches_on_worker_after_a_recorded_sonnet_feature_launch_until_both_arms_hold_fifteen_runs() {
    let feature = |number: u64| issue("makgora", number, "- [ ] a\nCategory: feature", &[]);
    let last = Record { tier: Some("sonnet-high".into()), category: Some("feature".into()), arm: true, name: "sc-1-d0800".into(), item: "makgora#1".into(), ..haiku_try("makgora#1", 50.0) };
    let launches = launched(vec![feature(2), feature(3)], History::new(&[last]), BTreeMap::new());
    assert_eq!(tiers(&launches), [("makgora#2", "worker"), ("makgora#3", "sonnet-high")]);
    assert!(launches.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
}

#[test]
fn dispatch_follows_a_fresh_tune_route_cell_and_decides_live_when_it_is_stale_or_absent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cells.json");
    std::fs::write(&path, r#"{"route tooling": "worker", "eta_minutes opus tooling": 30}"#).unwrap();
    let now = std::time::SystemTime::now();
    let category = |name: &'static str| move |key: &str| if key == "Category" { name.to_owned() } else { String::new() };
    let ledger = |routes: &BTreeMap<String, Tier>| ["tooling", "feature"].map(|name| (name.to_owned(), Evidence::default().tuned(routes, name))).into_iter().collect::<BTreeMap<_, _>>();
    let fresh = super::tuned::Tuned::read(&path, now).routes();
    assert_eq!(fresh, BTreeMap::from([("tooling".to_owned(), Tier::Opus)]));
    let tuned = super::tier::route_for("north#1", &category("tooling"), &History::default(), &ledger(&fresh));
    assert_eq!((tuned.tier, tuned.arm), (Tier::Opus, false), "{}", tuned.why);
    assert!(tuned.why.contains("north tune's route cell"), "{}", tuned.why);
    let untuned = super::tier::route_for("north#2", &category("feature"), &History::default(), &ledger(&fresh));
    assert_eq!((untuned.tier, untuned.arm), (Tier::Sonnet, true), "{}", untuned.why);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(now - std::time::Duration::from_secs(37 * 3_600)).unwrap();
    let stale = super::tuned::Tuned::read(&path, now).routes();
    assert!(stale.is_empty());
    let live = super::tier::route_for("north#1", &category("tooling"), &History::default(), &ledger(&stale));
    assert_eq!((live.tier, live.arm), (Tier::Sonnet, true), "{}", live.why);
    assert!(super::tuned::Tuned::read(&dir.path().join("absent.json"), now).routes().is_empty());
}

#[test]
fn a_reworked_landing_counts_against_its_tier_in_the_cost_per_closed_box_decision() {
    use crate::ledger::closed::{SpendRun, summarize};
    use crate::ledger::outcome::replay_tests::{judged, ledger, spend_runs};
    let db = ledger();
    let runs = spend_runs(&db, &judged(&db));
    let closed: std::collections::HashSet<String> = runs.iter().filter(|run| run.landed).filter_map(|run| run.item.clone()).collect();
    let prices = crate::router::pricing::PriceTable::load().unwrap();
    let report = |runs: &[SpendRun]| summarize(runs, &closed, "2026-10-28T12:00:00Z", &prices);
    let runs: Vec<SpendRun> = runs.into_iter().map(|run| SpendRun { arm: if run.tier.contains("sonnet") { "sonnet-high" } else { "worker" }.into(), ..run }).collect();
    let decide = |runs: &[SpendRun]| Evidence::default().counted(&stamped(15, 15)).priced(&report(runs), "feature");
    let unaware: Vec<SpendRun> = runs.iter().cloned().map(|run| SpendRun { reworked: false, ..run }).collect();
    assert_eq!(decide(&unaware).decided(), Some(Tier::Sonnet), "without rework Sonnet's two closed feature boxes cost $5.20 each against Opus's $13.87");
    let sonnet = report(&runs).cells.into_iter().find(|cell| cell.tier == "sonnet" && cell.category == "feature").unwrap();
    assert_eq!((sonnet.closed, sonnet.reworked, sonnet.kept()), (2, 2, 0), "one fixed forward, one reverted within 48 h");
    let aware = decide(&runs);
    assert_eq!(aware.decided(), Some(Tier::Opus));
    assert!(aware.figures(Tier::Sonnet).ends_with("every closed box reworked") && aware.figures(Tier::Opus).ends_with("$13.87 per closed box"), "{} / {}", aware.figures(Tier::Sonnet), aware.figures(Tier::Opus));
}


#[test]
fn an_unread_closed_report_or_arm_count_prints_one_tier_routing_degraded_line_each() {
    let mut notes = Vec::new();
    assert!(super::survey::read_closed(Err(crate::error::NorthError::Configuration("gh issue list tompassarelli/north: offline".into())), &mut notes).is_none());
    assert!(super::survey::read_arm_runs("feature", Err(crate::error::NorthError::Configuration("database is locked".into())), &mut notes).is_empty());
    assert_eq!(notes.len(), 2, "{notes:?}");
    assert!(notes[0].starts_with("tier routing degraded: north ledger summary --since 30d unread") && notes[0].ends_with("offline"), "{notes:?}");
    assert!(notes[1].starts_with("tier routing degraded: arm runs of feature unread") && notes[1].ends_with("database is locked"), "{notes:?}");
    let mut quiet = Vec::new();
    assert!(super::survey::read_closed(Ok(closed(&[("sonnet-high", 1, 1.0)])), &mut quiet).is_some() && quiet.is_empty());
}

#[test]
fn the_feature_arms_of_5_november_with_14_closed_boxes_each_keep_alternating_whatever_the_tier_wide_cells_say() {
    let arm_box = |arm: &str, number: usize, usd: f64, closed: bool, reworked: bool| ArmBox { item: format!("makgora#{arm}-{number}"), tier: String::new(), arm: arm.into(), usd, category: "feature".into(), closed, reworked };
    let sonnet = (0..23).map(|number| arm_box("sonnet-high", number, 123.75 / 23.0, number < 14, number < 2));
    let opus = (0..17).map(|number| arm_box("worker", number, 116.64 / 17.0, number < 14, false));
    let mut cells = vec![Cell::priced("sonnet", "feature", 82, 575.85), Cell::priced("opus-medium", "feature", 25, 578.93)];
    cells.iter_mut().for_each(|cell| cell.reworked = if cell.tier == "sonnet" { 9 } else { 2 });
    let report = ClosedReport { skipped: Vec::new(), closed: 107, cells, arms: sonnet.chain(opus).collect(), spend_usd: 0.0, leads_usd: 0.0, ..ClosedReport::default() };
    let evidence = Evidence::default().counted(&stamped(65, 67)).priced(&report, "feature");
    assert_eq!((evidence.figures(Tier::Sonnet).ends_with("14 of 15 closed arm boxes, $10.31 per closed box"), evidence.figures(Tier::Opus).ends_with("14 of 15 closed arm boxes, $8.33 per closed box")), (true, true), "{} / {}", evidence.figures(Tier::Sonnet), evidence.figures(Tier::Opus));
    assert_eq!(evidence.settled(), None, "14 closed boxes on each arm: the arms keep alternating");
    let feature = |number: u64| issue("makgora", number, "- [ ] a\nCategory: feature", &[]);
    let launches = launched(vec![feature(1), feature(2)], History::default(), BTreeMap::from([("feature".to_owned(), evidence)]));
    assert_eq!(tiers(&launches), [("makgora#1", "sonnet-high"), ("makgora#2", "worker")]);
}

fn effort_boxes(category: &str, arms: &[(&str, usize, usize, f64)]) -> ClosedReport {
    let arm_box = |arm: &str, number: usize, reworked: bool, usd: f64| ArmBox { item: format!("makgora#{arm}-{number}"), tier: "opus-medium".into(), arm: arm.into(), usd, category: category.into(), closed: true, reworked };
    let boxes = arms.iter().flat_map(|(arm, closed, reworked, usd)| (0..*closed).map(move |number| arm_box(arm, number, number < *reworked, *usd)));
    ClosedReport { skipped: Vec::new(), closed: 0, cells: Vec::new(), arms: boxes.collect(), spend_usd: 0.0, leads_usd: 0.0, boxes: Vec::new(), loaded: BTreeMap::new() }
}

fn opus_evidence(category: &str, opus_runs: usize, stamped: &[(&str, usize)], report: &ClosedReport) -> BTreeMap<String, Evidence> {
    let tiers = BTreeMap::from([("claude:opus-medium".to_owned(), (70.0, opus_runs))]);
    let stamped = stamped.iter().map(|(arm, runs)| ((*arm).to_owned(), *runs)).collect();
    BTreeMap::from([(category.to_owned(), Evidence { tiers, ..Evidence::default() }.counted(&stamped).priced(report, category))])
}

fn effort_launches(category: &str, ledger: BTreeMap<String, Evidence>) -> Vec<(super::plan::Launch, String)> {
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0), measure: None }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: slots::PlanSize { five_hour: 4.0, week: 2.0 }, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None, burn: None }];
    let boxes = (1..=3).map(|number| issue("makgora", number, &format!("- [ ] a\nCategory: {category}\nClass: a lost input"), &[])).collect();
    let mut survey = survey(boxes, Cloud::Closed(String::new()), hosts, accounts);
    survey.ledger = ledger;
    let command = |launch: super::plan::Launch| (effects::command(&launch, "sc-1", &survey, "/home/tom", "north").unwrap().1.join(" "), launch);
    decide(&survey, "test").launches.into_iter().map(command).map(|(command, launch)| (launch, command)).collect()
}

fn stamps(launches: &[(super::plan::Launch, String)]) -> Vec<(Option<&str>, bool)> {
    launches.iter().map(|(launch, command)| (launch.brief.lines().find_map(|line| line.strip_prefix("Arm: ")), command.contains("--effort high"))).collect()
}

#[test]
fn opus_boxes_alternate_medium_and_high_effort_arms_once_their_category_holds_fifteen_opus_runs() {
    let unread = effort_boxes("debugging-unknown-cause", &[]);
    let young = effort_launches("debugging-unknown-cause", opus_evidence("debugging-unknown-cause", 14, &[], &unread));
    assert!(young.iter().all(|(launch, command)| launch.route.effort.is_none() && command.contains("--effort medium") && launch.handoff.is_some()), "14 Opus runs: the tier's effort and the handoff arms");
    let armed = effort_launches("debugging-unknown-cause", opus_evidence("debugging-unknown-cause", 15, &[], &unread));
    assert_eq!(stamps(&armed), [(Some("effort-medium"), false), (Some("effort-high"), true), (Some("effort-medium"), false)], "15 Opus runs: the effort arms alternate, medium first, each on its stamp");
    assert!(armed.iter().all(|(launch, _)| launch.handoff.is_none()), "a box on an effort arm carries that arm alone, since the ledger records only the first Arm line");
    assert!(armed[1].0.route.why.contains("effort-medium 0 of 15 arm runs, 0 of 15 closed arm boxes, no closed box; effort-high 0 of 15 arm runs"), "{}", armed[1].0.route.why);
    let last = Record { tier: Some("worker".into()), category: Some("debugging-unknown-cause".into()), effort_arm: Some("effort-medium".into()), ..haiku_try("makgora#9", 50.0) };
    let route = super::tier::route_for("makgora#4", &|key| if key == "Category" { "debugging-unknown-cause".into() } else { String::new() }, &History::new(&[last]), &opus_evidence("debugging-unknown-cause", 15, &[], &unread));
    assert_eq!(route.stamp(), Some("effort-high"), "the next box takes the effort arm after the recorded one");
    let tooling = effort_launches("tooling", opus_evidence("tooling", 40, &[], &unread));
    assert!(tooling.iter().all(|(launch, _)| launch.route.effort.is_none()), "a box on a tier arm keeps the tier's effort");
}

#[test]
fn a_fixture_ledger_with_both_efforts_settles_the_cheaper_kept_box_effort_and_dispatch_launches_with_it() {
    let category = "debugging-unknown-cause";
    let full = [("effort-medium", 15), ("effort-high", 15)];
    let reworked = effort_boxes(category, &[("effort-medium", 15, 10, 5.0), ("effort-high", 15, 0, 8.0)]);
    let settled = effort_launches(category, opus_evidence(category, 40, &full, &reworked));
    assert_eq!(stamps(&settled), [(Some("handoff-150k"), true), (Some("no-handoff"), true), (Some("handoff-150k"), true)], "high at $8 a kept box beats medium at $5 a box but $15 a kept box once 10 of 15 are reworked, and a settled effort leaves the Arm line to the handoff arms");
    assert!(settled[0].0.route.why.contains("at high effort by the ledger's cost per kept box (effort-medium 15 of 15 arm runs, 15 of 15 closed arm boxes, $15.00 per kept box; effort-high 15 of 15 arm runs, 15 of 15 closed arm boxes, $8.00 per kept box)"), "{}", settled[0].0.route.why);
    let cheap = effort_boxes(category, &[("effort-medium", 15, 0, 5.0), ("effort-high", 15, 0, 8.0)]);
    assert!(effort_launches(category, opus_evidence(category, 40, &full, &cheap)).iter().all(|(launch, command)| launch.route.effort == Some(super::tier::Effort::Medium) && command.contains("--effort medium")), "medium when it is cheaper per kept box");
    let tie = effort_boxes(category, &[("effort-medium", 15, 0, 5.0), ("effort-high", 15, 0, 5.0)]);
    assert!(effort_launches(category, opus_evidence(category, 40, &full, &tie)).iter().all(|(_, command)| command.contains("--effort medium")), "medium on a tie");
    let short = effort_boxes(category, &[("effort-medium", 15, 10, 5.0), ("effort-high", 14, 0, 8.0)]);
    assert_eq!(stamps(&effort_launches(category, opus_evidence(category, 40, &full, &short)))[0], (Some("effort-medium"), false), "14 closed high boxes keep the effort arms alternating");
}

#[test]
fn the_tune_ticks_effort_cell_launches_its_effort_over_the_ledgers_own_reading() {
    let category = "debugging-unknown-cause";
    let dir = std::env::temp_dir().join(format!("north-effort-cell-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cells = dir.join("cells.json");
    std::fs::write(&cells, r#"{"effort debugging-unknown-cause": "high", "route tooling": "worker"}"#).unwrap();
    let efforts = super::tuned::Tuned::read(&cells, std::time::SystemTime::now()).efforts();
    assert_eq!(efforts, BTreeMap::from([(category.to_owned(), super::tier::Effort::High)]));
    let cheap = effort_boxes(category, &[("effort-medium", 15, 0, 5.0), ("effort-high", 15, 0, 8.0)]);
    let ledger = opus_evidence(category, 40, &[("effort-medium", 15), ("effort-high", 15)], &cheap).into_iter().map(|(name, evidence)| (name, evidence.tuned_effort(&efforts, category))).collect();
    let launches = effort_launches(category, ledger);
    assert!(launches.iter().all(|(launch, command)| command.contains("--effort high") && launch.route.why.contains("by north tune's effort cell")), "{:?}", launches[0].0.route.why);
    let stale = super::tuned::Tuned::read(&cells, std::time::SystemTime::now() + std::time::Duration::from_secs(37 * 3_600)).efforts();
    assert!(stale.is_empty(), "a cell older than 36 h routes nothing");
    std::fs::remove_dir_all(&dir).unwrap();
}
