use super::grounds::Ledger;
use super::tier::{Route, Tier};
use crate::ledger::{PriorReport, SpendRun, summarize};
use std::collections::HashSet;

const BODY: &str = "Category: tooling\n- [ ] a\n\n## Not required\n\n- No change to the spawn gate.\n- No new flags.\n\n## Notes\n\nNot required later.";
/// When wisp#83 was opened, 2026-10-29T15:15:27Z, as unix seconds.
const OPENED: f64 = 1_793_286_927.0;
/// 2026-10-30T10:00:00Z, after it opened.
const AFTER: f64 = 1_793_354_400.0;
/// 2026-10-08T12:00:00Z, when an earlier issue held the same item name.
const BEFORE: f64 = 1_791_460_800.0;

fn run(item: &str, minutes: u32) -> SpendRun {
    SpendRun {
        agent: String::new(),
        item: Some(item.into()),
        role: "worker".into(),
        tier: "sonnet".into(),
        category: "tooling".into(),
        arm: String::new(),
        model: Some("claude-sonnet-5-5".into()),
        started: "2026-10-27T10:00:00Z".into(),
        ended: format!("2026-10-27T{:02}:{:02}:00Z", 10 + minutes / 60, minutes % 60),
        landed: true,
        reworked: false,
        input: 3_000_000,
        cache_read: 2_000_000,
        output: 100_000,
        skills: Vec::new(),
    }
}

fn ledger(prior: &[(&str, Vec<PriorReport>)]) -> Ledger {
    let runs: Vec<SpendRun> = [("north#1", 10), ("north#2", 20), ("north#3", 30), ("north#4", 40)].map(|(item, minutes)| run(item, minutes)).into();
    let closed: HashSet<String> = runs.iter().filter_map(|run| run.item.clone()).collect();
    let report = summarize(&runs, &closed, "2026-10-27T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap());
    Ledger::new(Some(report), prior.iter().map(|(item, reports)| ((*item).to_owned(), reports.clone())).collect())
}

fn route() -> Route {
    Route { tier: Tier::Sonnet, category: "tooling".into(), arm: false, effort: None, effort_arm: false, follows: Some("north#240".into()), why: "tooling arm sonnet-high".into() }
}

fn reported(ended: f64, report: &str) -> PriorReport {
    PriorReport { ended, report: report.into() }
}

#[test]
fn a_brief_carries_no_prior_report_from_before_its_issue_was_opened() {
    let stale = reported(BEFORE, "Not done: makgora-dev#192's feature index waits on Tom");
    let at_opening = reported(OPENED, "Not done: reported the second the issue opened");
    let ledger = ledger(&[("wisp#83", vec![reported(AFTER, "Not done: the pair stayed leased"), at_opening, stale.clone(), stale])]);
    let grounds = ledger.grounds("wisp#83", OPENED, BODY, &route());
    assert_eq!(grounds.prior, ["Not done: the pair stayed leased"], "only reports that ended after the issue opened are carried");
    assert_eq!(ledger.grounds("wisp#83", BEFORE - 1.0, BODY, &route()).prior.len(), 2, "an issue opened before every report still carries its newest two");
}


