use super::{Seen, Settings};
use crate::dispatch::decide;
use crate::dispatch::tests::{host, issue, survey};
use crate::dispatch::tier::Evidence;
use crate::router::policy::ARM_RUNS;
use crate::router::settle::Outcome;
use std::collections::{BTreeMap, BTreeSet};

fn drawn_efforts(ledger: BTreeMap<String, Evidence>) -> Vec<(String, String)> {
    let hosts = vec![host("whiterabbit", 8, Ok(0))];
    let boxes: Vec<_> = (1..=8).map(|number| issue("north", number, &format!("- [ ] add column {number}"), &[])).collect();
    let mut planned = survey(boxes, hosts, Vec::new());
    planned.deepseek = Seen { settings: Settings::parse(&"[dispatch]\nhosts = [\"whiterabbit\"]\nslot_cap = 8\n".parse().unwrap()), tried: BTreeSet::new(), review: super::review::Found::default(), running: 0 };
    planned.ledger = ledger;
    let outcome = decide(&planned, "test");
    outcome
        .launches
        .iter()
        .map(|launch| {
            let (_, words) = crate::dispatch::effects::command(launch, "north-1-d0101", &planned, "/home/tom", "/runtime/bin/north").unwrap();
            let at = words.iter().position(|word| word == "--effort").expect("a DeepSeek launch passes its drawn effort");
            let stamp = launch.brief.lines().find_map(|line| line.strip_prefix("Arm: ")).expect("the brief carries the arm stamp the ledger reads").to_owned();
            (words[at + 1].clone(), stamp)
        })
        .collect()
}

fn arm_boxes(kept: bool, usd: f64) -> Vec<Outcome> {
    (0..ARM_RUNS).map(|_| Outcome { usd, closed: true, kept }).collect()
}

#[test]
fn opencode_launch_draws_high_or_max_effort_and_the_share_moves_toward_the_better_arm() {
    let level = drawn_efforts(BTreeMap::new());
    assert_eq!(level.len(), 8);
    assert!(level.iter().all(|(effort, stamp)| ["high", "max"].contains(&effort.as_str()) && stamp == &format!("deepseek-{effort}")), "{level:?}");
    assert!(level.iter().any(|(effort, _)| effort == "high") && level.iter().any(|(effort, _)| effort == "max"), "with no evidence both arms are drawn: {level:?}");

    let outcomes = BTreeMap::from([("deepseek-high".to_owned(), arm_boxes(true, 1.0)), ("deepseek-max".to_owned(), arm_boxes(false, 1.0))]);
    let evidence = Evidence { outcomes, cell_boxes: BTreeMap::new() };
    let shares = evidence.shares("feature", "deepseek");
    let share = |arm: &str| shares.arms.iter().find(|(name, _)| name == arm).map(|(_, share)| *share).unwrap();
    assert!(share("deepseek-high") > 0.5 && share("deepseek-high") > share("deepseek-max"), "{shares:?}");

    let favoured = drawn_efforts(BTreeMap::from([("feature".to_owned(), evidence)]));
    let high = favoured.iter().filter(|(effort, _)| effort == "high").count();
    assert!(high >= 6 && high > level.iter().filter(|(effort, _)| effort == "high").count(), "the better-landing arm takes the draws: {favoured:?}");
}
