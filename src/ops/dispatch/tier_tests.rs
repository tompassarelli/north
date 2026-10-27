use super::plan::Cloud;
use super::record::Record;
use super::tests::{issue, survey};
use super::tier::{ARM_RUNS, Evidence, History, Tier};
use super::{decide, effects, slots};
use std::collections::BTreeMap;

const RECOMMEND: &str = "tier claude:sonnet-high  cell [Category: tooling]  n 30
success threshold 80%  rule $ per success (no tier passes; lowest measured cost among sampled tiers)
  claude:sonnet-high 73% of 15  $0.21/success (priced n=15)
  claude:opus-medium 66% of 15  $0.60/success (priced n=15)
  claude-sonnet-5-5 medium 33% of 6  $0.09/success (priced n=5)
eta 32m (p75 of n=15, cell Category: tooling, tier claude:sonnet-high)";

fn launched(boxes: Vec<super::issues::Issue>, history: History, ledger: BTreeMap<String, Evidence>) -> Vec<(String, Tier, bool, String)> {
    let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0) }];
    let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: 4.0, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None }];
    let mut survey = survey(boxes, Cloud::Closed(String::new()), hosts, accounts);
    (survey.history, survey.ledger) = (history, ledger);
    decide(&survey, "test").launches.into_iter().map(|launch| (launch.item.to_string(), launch.route.tier, launch.route.arm, launch.brief)).collect()
}

fn tiers(launches: &[(String, Tier, bool, String)]) -> Vec<(&str, &str)> {
    launches.iter().map(|(item, tier, _, _)| (item.as_str(), tier.name())).collect()
}

fn haiku_try(item: &str, at: f64) -> Record {
    Record { at, item: item.into(), host: "gex45".into(), account: "claude:b".into(), name: "sc-3-d0900".into(), cloud: false, launched: true, detail: String::new(), tier: Some("worker-haiku".into()), category: Some("mechanical".into()), arm: false }
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
    assert!(launches.iter().all(|(_, _, arm, brief)| !arm && !brief.contains("Arm:")), "only tooling and features run as arms");
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
fn tooling_and_features_alternate_recorded_arms_until_each_has_fifteen_runs_then_take_the_ledgers_pick() {
    let tooling = |number: u64| issue("wisp", number, "- [ ] a\nCategory: tooling", &[]);
    let launches = launched(vec![tooling(1), tooling(2), tooling(3), issue("wisp", 4, "- [ ] a", &[])], History::default(), BTreeMap::new());
    assert_eq!(tiers(&launches), [("wisp#1", "sonnet-high"), ("wisp#2", "worker"), ("wisp#3", "sonnet-high"), ("wisp#4", "sonnet-high")], "an undeclared category is a feature, with its own arms");
    assert!(launches.iter().all(|(_, tier, arm, brief)| *arm && brief.contains(&format!("Arm: {}", tier.name()))));
    let last = Record { tier: Some("sonnet-high".into()), category: Some("tooling".into()), arm: true, name: "wisp-1-d0800".into(), item: "wisp#1".into(), ..haiku_try("wisp#1", 50.0) };
    assert_eq!(tiers(&launched(vec![tooling(5)], History::new(&[last]), BTreeMap::new())), [("wisp#5", "worker")], "the next run takes the arm after the recorded one");
    let evidence = Evidence::parse(RECOMMEND, "tooling");
    assert_eq!((evidence.tiers["claude:sonnet-high"], evidence.tiers["claude:opus-medium"], evidence.tiers["claude-sonnet-5-5 medium"]), ((73.0, ARM_RUNS), (66.0, ARM_RUNS), (33.0, 6)));
    let decided = launched(vec![tooling(6), tooling(7)], History::default(), BTreeMap::from([("tooling".to_owned(), evidence.clone())]));
    assert_eq!(tiers(&decided), [("wisp#6", "sonnet-high"), ("wisp#7", "sonnet-high")], "both arms at 15 runs: the ledger's pick, every time");
    assert!(decided.iter().all(|(_, _, arm, brief)| !arm && !brief.contains("Arm:")));
    let short = Evidence { tiers: BTreeMap::from([("claude:sonnet-high".to_owned(), (73.0, 15)), ("claude:opus-medium".to_owned(), (66.0, 14))]), ..evidence.clone() };
    assert_eq!(tiers(&launched(vec![tooling(8), tooling(9)], History::default(), BTreeMap::from([("tooling".to_owned(), short)]))), [("wisp#8", "sonnet-high"), ("wisp#9", "worker")]);
    assert_eq!(Evidence::parse(&RECOMMEND.replace("[Category: tooling]", "[all runs]"), "tooling"), Evidence::default(), "a coarser cell is not this category's evidence");
    let ledger = |printed: String| BTreeMap::from([("tooling".to_owned(), Evidence::parse(&printed, "tooling"))]);
    let haiku = RECOMMEND.replace("tier claude:sonnet-high", "tier claude:haiku").replace("73% of 15", "60% of 15");
    assert_eq!(tiers(&launched(vec![tooling(10)], History::default(), ledger(haiku))), [("wisp#10", "worker")], "a pick outside the arms takes the arm with the higher success rate");
    let unsure = RECOMMEND.replace("tier claude:sonnet-high", "insufficient evidence");
    assert_eq!(Evidence::parse(&unsure, "tooling").pick, None);
    assert_eq!(tiers(&launched(vec![tooling(11)], History::default(), ledger(unsure))), [("wisp#11", "sonnet-high")], "insufficient evidence still counts both arms' runs, so they stop alternating");
}

#[test]
fn a_launch_runs_its_tiers_model_and_effort_and_an_unnamed_tier_fails_it() {
    let launches = {
        let hosts = vec![slots::Host { name: "gex45".into(), cap: Some(24), running: Ok(0) }];
        let accounts = vec![slots::Account { name: "claude:b".into(), hosts: vec!["gex45".into()], plan: 4.0, room: slots::Room { five_hour: 90.0, week: 90.0, week_reset_h: 100.0 }, spending: false, refusal: None }];
        let survey = survey(vec![issue("smashcraft", 3, "- [ ] a\nCategory: mechanical", &[]), issue("smashcraft", 1, "- [ ] a\nCategory: tooling", &[])], Cloud::Closed(String::new()), hosts, accounts);
        let outcome = decide(&survey, "test");
        outcome.launches.iter().map(|launch| effects::command(launch, "sc-1", &survey, "/home/tom", "north").unwrap().1.join(" ")).collect::<Vec<_>>()
    };
    assert!(launches[0].contains("--model claude-sonnet-5-5 --effort high"), "{}", launches[0]);
    assert!(launches[1].contains("--model claude-haiku-5-5 --effort high"), "{}", launches[1]);
    let bare = survey(Vec::new(), Cloud::Closed(String::new()), Vec::new(), Vec::new());
    let bare = super::survey::Survey { cfg: "[hosts.gex45]\n".parse().unwrap(), ..bare };
    let launch = super::plan::Launch { item: issue("smashcraft", 1, "", &[]).reference(), host: "gex45".into(), account: "claude:b".into(), brief: String::new(), route: route(Tier::Opus), cloud: false, local: None, pin: None };
    assert!(effects::command(&launch, "sc-1", &bare, "/home/tom", "north").unwrap_err().contains("claude:opus-medium"), "a tier orchestration does not name fails the launch, never falls back to the default model");
}

pub(super) fn route(tier: Tier) -> super::tier::Route {
    super::tier::Route { tier, category: "feature".into(), arm: false, follows: None, why: String::new() }
}
