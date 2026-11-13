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
        box_text: None,
        model: Some("claude-sonnet-5-5".into()),
        effort: String::new(),
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
    let report = summarize(&runs, &closed, &Default::default(), "2026-10-27T00:00:00Z", &crate::router::pricing::PriceTable::load().unwrap());
    Ledger::new(Some(report), prior.iter().map(|(item, reports)| ((*item).to_owned(), reports.clone())).collect())
}

fn route() -> Route {
    Route { tier: Tier::Sonnet, category: "tooling".into(), effort: None, effort_arm: false, cell: None, cell_role: None, signal: None, follows: Some("north#240".into()), why: "tooling arm sonnet-high".into() }
}

fn reported(ended: f64, report: &str) -> PriorReport {
    PriorReport { ended, report: report.into() }
}

#[test]
fn a_brief_carries_no_prior_report_from_before_its_issue_was_opened() {
    let stale = reported(BEFORE, "Not done: makgora-dev#192's feature index waits on Tom");
    let at_opening = reported(OPENED, "Not done: reported the second the issue opened");
    let ledger = ledger(&[("wisp#83", vec![reported(AFTER, "Not done: the pair stayed leased"), at_opening, stale.clone(), stale])]);
    let grounds = ledger.grounds("wisp#83", OPENED, BODY, &route(), "sonnet");
    assert_eq!(grounds.prior, ["Not done: the pair stayed leased"], "only reports that ended after the issue opened are carried");
    assert_eq!(ledger.grounds("wisp#83", BEFORE - 1.0, BODY, &route(), "sonnet").prior.len(), 2, "an issue opened before every report still carries its newest two");
}

/// n1054-d1217: dispatch carried north#722's report naming /home/tom/code/north/worktrees/n722-gate-debt into a brief after
/// the lane was gone, so the next worker was sent to a lane this host no longer held.
#[test]
fn a_prior_report_naming_a_gone_lane_path_is_dropped_from_the_brief() {
    let scratch = tempfile::tempdir().unwrap();
    let home = scratch.path();
    let gone = home.join("code/north/worktrees/n722-gate-debt");
    let here = home.join("code/north/worktrees/n1054-carried-report");
    std::fs::create_dir_all(&here).unwrap();
    let report = format!("Not done: the lanes at {} and {} still hold the work", gone.display(), here.display());
    let grounds = super::grounds::Grounds { home: Some(home.to_path_buf()), ..super::grounds::Grounds::default() }.carrying(Some(&report));
    let brief = super::brief::brief(&super::tests::issue("north", 1054, "- [ ] a", &[]), "dispatch", &route(), &grounds, None);

    assert!(!brief.contains(&gone.display().to_string()), "a gone lane's path still reaches the worker: {brief}");
    assert!(brief.contains(&format!(" and {} still hold the work", here.display())), "a lane this host holds stays named: {brief}");
    assert!(brief.contains("Prior report: Not done: the lanes at [absent on this host]"), "{brief}");
}

