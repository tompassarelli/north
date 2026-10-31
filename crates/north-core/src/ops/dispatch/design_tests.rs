use super::facets::{category, design};
use super::grounds::{Grounds, Ledger};
use super::issues::Issue;
use super::manual::{Ask, start};
use super::tier::{self, Evidence, History, Route, Tier};
use crate::ledger::{SpendRun, summarize};
use std::collections::{BTreeMap, HashSet};

/// wisp#81's layering box as dispatch read it on 2026-10-30.
const WISP_81: &str = "- [ ] cli -> os (3 layer violations): host/cli/cli.ts and host/cli/command.ts use os/platform Platform and platformLayer while os uses cli (hostProcess). Move process spawning into os and let cli use os, or inject the platform layer at the composition root.\n- [ ] test/runtime.test.ts pins \"4 of 4\" and \"6 of 6\" passed counts; assert passed equals total.";
const CFG: &str = "[tiers.\"claude:haiku\"]\nmodel = \"claude-haiku-5-5\"\neffort = \"high\"\n[tiers.\"claude:sonnet-high\"]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\n[tiers.\"claude:opus-medium\"]\nmodel = \"claude-opus-5-5\"\neffort = \"medium\"\n";

fn mechanical(body: &str) -> Issue {
    Issue { repo: "wisp".into(), number: 81, title: "code-health follow-ups".into(), body: format!("Category: mechanical\n{body}"), ..Issue::default() }
}

fn run(item: &str, tier: &str, model: &str, category: &str, input: i64) -> SpendRun {
    SpendRun {
        item: Some(item.into()),
        role: "worker".into(),
        tier: tier.into(),
        category: category.into(),
        arm: String::new(),
        model: Some(model.into()),
        started: "2026-10-27T10:00:00Z".into(),
        ended: "2026-10-27T10:20:00Z".into(),
        landed: true,
        input,
        cache_read: 0,
        output: 1_000,
    }
}

/// Five trivial Haiku boxes in the mechanical cell beside six real Opus feature boxes.
fn ledger() -> Ledger {
    let trivial = (1..=5).map(|n| run(&format!("wisp#{n}"), "haiku", "claude-haiku-5-5", "mechanical", 10_000));
    let real = (6..=11).map(|n| run(&format!("north#{n}"), "opus-medium", "claude-opus-5-5", "feature", 1_000_000));
    let runs: Vec<SpendRun> = trivial.chain(real).collect();
    let closed: HashSet<String> = runs.iter().filter_map(|run| run.item.clone()).collect();
    Ledger::new(Some(summarize(&runs, &closed, "2026-10-27T00:00:00Z", &crate::ledger::pricing::PriceTable::load().unwrap())), BTreeMap::new())
}

fn spend(budget: &str) -> f64 {
    budget.split_once(" at $").and_then(|(_, rest)| rest.split_whitespace().next()).and_then(|usd| usd.parse().ok()).unwrap()
}

#[test]
fn a_box_naming_a_layer_violation_a_new_module_or_a_design_option_is_not_categorized_mechanical() {
    assert_eq!(category(&mechanical(WISP_81)).as_deref(), Some("feature"));
    assert_ne!(tier::route(&mechanical(WISP_81), &History::default(), &BTreeMap::new()).tier, Tier::Haiku, "wisp#81 does not go to worker-haiku");
    assert_eq!(design("- [ ] Create a neutral module that owns the Platform contract"), Some("a new module"));
    assert_eq!(design("- [ ] Pick a design option for the relay"), Some("a design option"));
    assert_eq!(design("* [ ] take option (a) from the review"), Some("a design option"));
    let plain = mechanical("- [ ] Rename `lanPoolStop` to `stopLanPool` in host/cli.");
    assert_eq!((category(&plain).as_deref(), tier::route(&plain, &History::default(), &BTreeMap::new()).tier), (Some("mechanical"), Tier::Haiku), "a rename stays mechanical");
    assert_eq!(category(&mechanical("- [x] cli -> os (3 layer violations)\nNot required: a new module")).as_deref(), Some("mechanical"), "only unticked box text counts");
}

#[test]
fn a_hand_launch_declaring_wisp_81_mechanical_runs_as_a_feature() {
    let brief = "Item: wisp#81 code-health follow-ups\nCategory: mechanical\n\nDo it.";
    let ask = Ask { item: "wisp#81", brief, prompt: brief, model: None, reason: None };
    let started = start(&ask, WISP_81, &CFG.parse().unwrap(), &History::default(), &BTreeMap::<String, Evidence>::new(), &|_| Grounds::default()).unwrap();
    assert_eq!((started.route.category.as_str(), started.route.tier == Tier::Haiku), ("feature", false));
    assert!(started.prompt.starts_with("Item: wisp#81 code-health follow-ups\nCategory: feature\nTier: sonnet-high\n") && !started.prompt.contains("mechanical"), "{}", started.prompt);
}

#[test]
fn a_cell_whose_few_closed_boxes_were_trivial_budgets_the_median_closed_box_across_every_cell() {
    let ledger = ledger();
    let haiku = Route { tier: Tier::Haiku, category: "mechanical".into(), arm: false, follows: None, why: String::new() };
    let grounds = ledger.grounds("wisp#81", 0.0, WISP_81, &haiku);
    let (cell, (floor, boxes)) = (grounds.figures.clone().unwrap(), grounds.floor.unwrap());
    assert!(cell.median_usd * 2.0 < floor && boxes == 11, "the haiku cell's ${:.2} sits below the ${floor:.2} floor of {boxes} boxes", cell.median_usd);
    let budget = grounds.budget(&haiku).unwrap();
    assert_eq!(spend(&budget), (floor * 100.0).round() / 100.0, "{budget}");
    assert!(budget.contains("the median cost of a closed box across every cell (n=11 boxes, ledger 7d)") && budget.contains("n=5 boxes, cell mechanical on haiku"), "{budget}");
    let opus = Route { tier: Tier::Opus, category: "feature".into(), ..haiku };
    let real = ledger.grounds("north#12", 0.0, "", &opus);
    let budget = real.budget(&opus).unwrap();
    assert_eq!(spend(&budget), (real.figures.unwrap().median_usd * 200.0).round() / 100.0, "a cell above the floor keeps twice its median: {budget}");
}
