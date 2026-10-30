use super::brief::{brief, cloud};
use super::grounds::{Grounds, Ledger, not_required};
use super::manual::{Ask, start};
use super::tests::{ORDERS, issue};
use super::tier::{Evidence, History, Route, Tier};
use crate::ledger::{PriorReport, SpendRun, summarize};
use std::collections::{BTreeMap, HashSet};

const BODY: &str = "Category: tooling\n- [ ] a\n\n## Not required\n\n- No change to the spawn gate.\n- No new flags.\n\n## Notes\n\nNot required later.";
const CFG: &str = "[tiers.\"claude:sonnet-high\"]\nmodel = \"claude-sonnet-5-5\"\neffort = \"high\"\n";
/// When wisp#83 was opened, 2026-10-29T15:15:27Z, as unix seconds.
const OPENED: f64 = 1_793_286_927.0;
/// 2026-10-30T10:00:00Z, after it opened.
const AFTER: f64 = 1_793_354_400.0;
/// 2026-10-08T12:00:00Z, when an earlier issue held the same item name.
const BEFORE: f64 = 1_791_460_800.0;

fn run(item: &str, minutes: u32) -> SpendRun {
    SpendRun {
        item: Some(item.into()),
        role: "worker".into(),
        tier: "sonnet".into(),
        category: "tooling".into(),
        arm: String::new(),
        model: Some("claude-sonnet-5-5".into()),
        started: "2026-10-27T10:00:00Z".into(),
        ended: format!("2026-10-27T{:02}:{:02}:00Z", 10 + minutes / 60, minutes % 60),
        landed: true,
        input: 3_000_000,
        cache_read: 2_000_000,
        output: 100_000,
    }
}

fn ledger(prior: &[(&str, Vec<PriorReport>)]) -> Ledger {
    let runs: Vec<SpendRun> = [("north#1", 10), ("north#2", 20), ("north#3", 30), ("north#4", 40)].map(|(item, minutes)| run(item, minutes)).into();
    let closed: HashSet<String> = runs.iter().filter_map(|run| run.item.clone()).collect();
    let report = summarize(&runs, &closed, "2026-10-27T00:00:00Z", &crate::ledger::pricing::PriceTable::load().unwrap());
    Ledger::new(Some(report), prior.iter().map(|(item, reports)| ((*item).to_owned(), reports.clone())).collect())
}

fn route() -> Route {
    Route { tier: Tier::Sonnet, category: "tooling".into(), arm: false, follows: Some("north#240".into()), why: "tooling arm sonnet-high".into() }
}

fn reported(ended: f64, report: &str) -> PriorReport {
    PriorReport { ended, report: report.into() }
}

fn priors() -> Vec<PriorReport> {
    vec![reported(AFTER + 60.0, &format!("Not done: {}", "x".repeat(900))), reported(AFTER, "Not done: the gate refused the push")]
}

fn carries(text: &str, why: Option<&str>) {
    for needed in [
        "Not required: No change to the spawn gate.",
        "Not required: No new flags.",
        "Prior report: Not done: the gate refused the push",
        "ETA: 30 min (p75 of n=4 runs, cell tooling on sonnet, ledger 7d)",
        "Budget: stop and report `Not done:` with where you are at $10.40 API-equivalent spend, twice the cell's median cost per closed box (median $5.20 of n=4 boxes, cell tooling on sonnet, ledger 7d)",
    ] {
        assert!(text.contains(needed), "{needed:?} missing from {text}");
    }
    let long = text.lines().find(|line| line.starts_with("Prior report: Not done: xxx")).unwrap();
    assert_eq!(long.chars().count(), "Prior report: ".len() + 600, "a prior report is cut to 600 characters");
    if let Some(why) = why {
        assert!(text.contains(&format!("Why: {why}; the ETA and budget are the ledger's closed boxes of the last 7d")), "{text}");
    }
    assert!(!text.contains("Not required later"), "only the Not required section counts");
}

#[test]
fn a_dispatch_brief_carries_the_not_required_lines_two_prior_reports_the_ledgers_p75_eta_and_a_budget() {
    let ledger = ledger(&[("north#7", priors())]);
    let box_ = issue("north", 7, BODY, &[]);
    let grounds = ledger.grounds("north#7", OPENED, &box_.body, &route());
    let text = brief(&box_, "test", &route(), &grounds);
    carries(&text, Some("tooling arm sonnet-high"));
    assert!(text.contains("Follows: north#240"), "the trace names the box's Follows line: {text}");
    assert!(text.split("\n\n").next().unwrap().contains("Budget:"), "the lines sit in the header block placement reads");
}

#[test]
fn a_brief_carries_no_prior_report_from_before_its_issue_was_opened() {
    let stale = reported(BEFORE, "Not done: smashcraft-dev#192's feature index waits on Tom");
    let at_opening = reported(OPENED, "Not done: reported the second the issue opened");
    let ledger = ledger(&[("wisp#83", vec![reported(AFTER, "Not done: the pair stayed leased"), at_opening, stale.clone(), stale])]);
    let grounds = ledger.grounds("wisp#83", OPENED, BODY, &route());
    assert_eq!(grounds.prior, ["Not done: the pair stayed leased"], "only reports that ended after the issue opened are carried");
    assert_eq!(ledger.grounds("wisp#83", BEFORE - 1.0, BODY, &route()).prior.len(), 2, "an issue opened before every report still carries its newest two");
}

#[test]
fn a_hand_launch_brief_carries_the_same_four_things() {
    let ledger = ledger(&[("north#7", priors())]);
    let ask = Ask { item: "north#7", brief: "Item: north#7 box\nCategory: tooling\nETA: 75 min\n\nDo it.", prompt: "Item: north#7 box\nCategory: tooling\nETA: 75 min\n\nDo it.", model: None, reason: None };
    let started = start(&ask, &CFG.parse().unwrap(), &History::default(), &BTreeMap::<String, Evidence>::new(), &|route| ledger.grounds("north#7", OPENED, BODY, route)).unwrap();
    assert_eq!(started.route.tier, Tier::Sonnet);
    carries(&started.prompt, None);
    assert!(started.prompt.contains("ETA: 75 min\n") && started.prompt.contains("Ledger ETA: 30 min (p75 of n=4 runs"), "{}", started.prompt);
    assert!(started.prompt.ends_with("\n\nDo it."), "the lead's text after the header is kept");
}

#[test]
fn a_hand_launch_with_a_named_tier_still_carries_the_trace_and_a_model_override_carries_no_measured_figures() {
    let ledger = ledger(&[("north#7", priors())]);
    let named = "Item: north#7 box\nCategory: tooling\nTier: sonnet-high\n\nDo it.";
    let ask = Ask { item: "north#7", brief: named, prompt: named, model: None, reason: None };
    let started = start(&ask, &CFG.parse().unwrap(), &History::default(), &BTreeMap::<String, Evidence>::new(), &|route| ledger.grounds("north#7", OPENED, BODY, route)).unwrap();
    assert!(started.prompt.contains("Not required: No new flags.") && started.prompt.contains("Budget: stop and report"), "{}", started.prompt);
    assert!(!started.prompt.contains("Arm:") && started.prompt.matches("Tier:").count() == 1, "a named tier is not stamped again: {}", started.prompt);
    let overridden = Ask { model: Some("opus"), reason: Some("spans two engines"), ..ask };
    let started = start(&overridden, &CFG.parse().unwrap(), &History::default(), &BTreeMap::<String, Evidence>::new(), &|route| ledger.grounds("north#7", OPENED, BODY, route)).unwrap();
    assert!(started.prompt.contains("Not required: No new flags.") && !started.prompt.contains("Budget:") && !started.prompt.contains("ETA:"), "{}", started.prompt);
}

#[test]
fn a_cell_with_no_closed_box_keeps_the_declared_eta_or_the_uncalibrated_default_and_names_no_budget() {
    let route = Route { category: "feature".into(), ..route() };
    let empty = ledger(&[]).grounds("north#7", OPENED, BODY, &route);
    assert_eq!(empty.figures, None);
    let declared = brief(&issue("north", 7, "Category: feature\nETA: 60 min\n- [ ] a", &[]), "test", &route, &empty);
    assert!(declared.contains("ETA: 60 min\n") && !declared.contains("Budget:") && declared.contains("unmeasured for this cell"), "{declared}");
    let bare = brief(&issue("north", 7, "- [ ] a", &[]), "test", &route, &Grounds::default());
    assert!(bare.contains("ETA: 45 min (uncalibrated)"), "{bare}");
}

#[test]
fn the_eta_and_budget_change_with_the_ledgers_closed_boxes() {
    let slow: Vec<SpendRun> = [("north#1", 50), ("north#2", 60)].map(|(item, minutes)| run(item, minutes)).into();
    let closed: HashSet<String> = slow.iter().filter_map(|run| run.item.clone()).collect();
    let report = summarize(&slow, &closed, "2026-10-27T00:00:00Z", &crate::ledger::pricing::PriceTable::load().unwrap());
    let later = Ledger::new(Some(report), BTreeMap::new()).grounds("north#7", OPENED, BODY, &route());
    let first = ledger(&[]).grounds("north#7", OPENED, BODY, &route());
    assert!(later.eta(None, &route()).starts_with("60 min (p75 of n=2 runs") && first.eta(None, &route()).starts_with("30 min (p75 of n=4 runs"));
}

#[test]
fn a_cloud_brief_carries_the_standing_orders() {
    let text = cloud("Item: north#9 x", "north", "claude/land-n9", ORDERS);
    assert!(text.contains(&format!("Standing orders, which hold in the cloud too:\n{ORDERS}")), "{text}");
}

#[test]
fn not_required_reads_its_section_and_its_inline_lines() {
    assert_eq!(not_required(BODY), ["No change to the spawn gate.", "No new flags."]);
    assert_eq!(not_required("- [ ] a\nNot required: the launcher\n- **Not required:** docs"), ["the launcher", "docs"]);
    assert!(not_required("- [ ] a\nNothing here").is_empty());
}

