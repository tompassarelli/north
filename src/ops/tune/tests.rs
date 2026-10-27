use super::cells::Cells;
use super::{Ledger, Tick, Tracker, tick};
use crate::ledger::pricing::PriceTable;
use crate::ledger::{ClosedReport, SpendRun, summarize};
use crate::ops::dispatch::Evidence;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

const AT: &str = "2026-10-27T06:00:00Z";
const SONNET: (&str, &str) = ("sonnet", "claude-sonnet-5-5");
const OPUS: (&str, &str) = ("medium", "claude-opus-5-5");

fn run(item: &str, (tier, model): (&str, &str), category: &str, started: &str, ended: &str) -> SpendRun {
    SpendRun {
        item: Some(item.into()),
        role: "worker".into(),
        tier: tier.into(),
        category: category.into(),
        model: Some(model.into()),
        started: started.into(),
        ended: ended.into(),
        landed: true,
        input: 3_000_000,
        cache_read: 2_000_000,
        output: 100_000,
    }
}

fn report(runs: &[SpendRun], since: &str) -> ClosedReport {
    let closed: HashSet<String> = runs.iter().filter_map(|run| run.item.clone()).collect();
    summarize(runs, &closed, since, &PriceTable::load().unwrap())
}

/// Sonnet closes two feature boxes at one run each and Opus one at two runs; Sonnet closes three
/// tooling boxes at two runs each, which cost one run each the week before, and Opus's one feature
/// box cost one run the week before.
fn ledger() -> Ledger {
    let week = report(
        &[
            run("north#1", SONNET, "feature", "2026-10-21T10:00:00Z", "2026-10-21T10:30:00Z"),
            run("north#2", SONNET, "feature", "2026-10-22T10:00:00Z", "2026-10-22T11:00:00Z"),
            run("north#3", OPUS, "feature", "2026-10-22T12:00:00Z", "2026-10-22T12:20:00Z"),
            run("north#3", OPUS, "feature", "2026-10-22T13:00:00Z", "2026-10-22T13:40:00Z"),
            run("north#4", SONNET, "tooling", "2026-10-23T10:00:00Z", "2026-10-23T10:10:00Z"),
            run("north#4", SONNET, "tooling", "2026-10-23T11:00:00Z", "2026-10-23T11:15:00Z"),
            run("north#8", SONNET, "tooling", "2026-10-24T10:00:00Z", "2026-10-24T10:10:00Z"),
            run("north#8", SONNET, "tooling", "2026-10-24T11:00:00Z", "2026-10-24T11:15:00Z"),
            run("north#9", SONNET, "tooling", "2026-10-25T10:00:00Z", "2026-10-25T10:10:00Z"),
            run("north#9", SONNET, "tooling", "2026-10-25T11:00:00Z", "2026-10-25T11:15:00Z"),
        ],
        "2026-10-20T06:00:00Z",
    );
    let before = report(
        &[
            run("north#5", SONNET, "tooling", "2026-10-15T10:00:00Z", "2026-10-15T10:10:00Z"),
            run("north#10", SONNET, "tooling", "2026-10-16T10:00:00Z", "2026-10-16T10:10:00Z"),
            run("north#11", SONNET, "tooling", "2026-10-17T10:00:00Z", "2026-10-17T10:10:00Z"),
            run("north#6", OPUS, "feature", "2026-10-15T12:00:00Z", "2026-10-15T12:20:00Z"),
            run("north#7", SONNET, "feature", "2026-10-16T10:00:00Z", "2026-10-16T10:30:00Z"),
        ],
        "2026-10-13T06:00:00Z",
    );
    let arms = |sonnet: (f64, usize), opus: (f64, usize)| BTreeMap::from([("claude:sonnet-high".to_owned(), sonnet), ("claude:opus-medium".to_owned(), opus)]);
    let evidence = BTreeMap::from([
        ("feature".to_owned(), Evidence { tiers: arms((60.0, 15), (70.0, 15)), usd_per_closed: None }.priced(&week, "feature")),
        ("tooling".to_owned(), Evidence { tiers: arms((80.0, 4), (0.0, 0)), usd_per_closed: None }.priced(&week, "tooling")),
    ]);
    Ledger { week, before, evidence, notes: Vec::new() }
}

#[derive(Default)]
struct Issues {
    open: Vec<String>,
    closed: Vec<String>,
    filed: Vec<(String, String)>,
    unreadable: bool,
    refuses: bool,
}

impl Tracker for Issues {
    fn open(&self) -> Result<Vec<String>, String> {
        if self.unreadable {
            return Err("gh: not logged in".into());
        }
        Ok(self.open.clone())
    }

    fn recently_closed(&self) -> Result<Vec<String>, String> {
        Ok(self.closed.clone())
    }

    fn file(&mut self, title: &str, body: &str) -> Result<String, String> {
        if self.refuses {
            return Err("gh issue create: HTTP 403".into());
        }
        self.filed.push((title.to_owned(), body.to_owned()));
        Ok(format!("https://github.com/tompassarelli/north/issues/{}", 900 + self.filed.len()))
    }
}

fn first_tick(issues: &mut Issues) -> Tick {
    tick(&ledger(), &Cells::new(), issues, AT, false)
}

#[test]
fn the_tick_prints_the_dashboard_line_first_and_the_open_north_issue_count_last() {
    let ledger = ledger();
    let mut issues = Issues { open: vec!["a".into(), "b".into()], ..Issues::default() };
    let tick = tick(&ledger, &Cells::new(), &mut issues, AT, false);
    assert_eq!(tick.lines[0], ledger.week.dashboard("7d"));
    assert!(tick.lines[0].starts_with("cost/closed 7d: all $"), "{}", tick.lines[0]);
    assert_eq!(tick.lines.last().unwrap(), "open north issues: 3", "the two open and the one filed: {:#?}", tick.lines);
    assert!(!tick.failed);
}

#[test]
fn routing_cells_are_written_only_for_categories_whose_arms_decided_beside_every_eta_and_budget_cell() {
    let tick = first_tick(&mut Issues::default());
    let cells: Vec<(&str, &Value)> = tick.cells.iter().map(|(cell, value)| (cell.as_str(), value)).collect();
    assert_eq!(
        cells,
        [
            ("budget_usd opus feature", &json!(41.6)),
            ("budget_usd sonnet feature", &json!(10.4)),
            ("budget_usd sonnet tooling", &json!(20.8)),
            ("eta_minutes opus feature", &json!(40.0)),
            ("eta_minutes sonnet feature", &json!(60.0)),
            ("eta_minutes sonnet tooling", &json!(15.0)),
            ("route feature", &json!("sonnet-high")),
        ],
        "Sonnet's $5.20 per closed box beats Opus's $20.80 on feature at 15 runs each; tooling's arms hold 4 and 0 runs, so no route"
    );
}

#[test]
fn each_change_records_its_rows_n_and_interval_and_an_unchanged_tick_records_none() {
    let mut issues = Issues::default();
    let first = first_tick(&mut issues);
    assert_eq!(first.records.len(), first.cells.len(), "a first tick records every cell");
    let route = first.records.iter().find(|record| record["cell"] == "route feature").unwrap();
    assert_eq!((&route["to"], &route["from"], &route["rows"], &route["n"], &route["interval"]), (&json!("sonnet-high"), &Value::Null, &json!(30), &json!(2), &json!([5.2, 5.2])));
    assert_eq!((&route["origin"], &route["pointer"], &route["at"]), (&json!("experiment"), &json!("north ledger summary --since 7d"), &json!(AT)));
    let eta = first.records.iter().find(|record| record["cell"] == "eta_minutes sonnet feature").unwrap();
    assert_eq!((&eta["rows"], &eta["n"], &eta["interval"]), (&json!(2), &json!(2), &json!([30.0, 60.0])));

    let again = tick(&ledger(), &first.cells, &mut issues, AT, false);
    assert!(again.records.is_empty(), "{:?}", again.records);
    assert!(again.lines.contains(&"no cell changed (7 cells)".to_owned()), "{:#?}", again.lines);

    let mut prior = first.cells.clone();
    prior.insert("budget_usd sonnet feature".into(), json!(8.0));
    prior.insert("route tooling".into(), json!("worker"));
    let moved = tick(&ledger(), &prior, &mut issues, AT, false);
    let changed: Vec<(&Value, &Value, &Value)> = moved.records.iter().map(|record| (&record["cell"], &record["from"], &record["to"])).collect();
    assert_eq!(changed, [(&json!("budget_usd sonnet feature"), &json!(8.0), &json!(10.4)), (&json!("route tooling"), &json!("worker"), &Value::Null)]);
    assert!(moved.lines.iter().any(|line| line.starts_with("changed budget_usd sonnet feature: 8.0 -> 10.4 (rows 2, n 2, interval [10.4, 10.4];")), "{:#?}", moved.lines);
}

#[test]
fn a_cell_whose_cost_per_closed_box_rose_over_a_quarter_files_one_issue_and_an_open_one_holds_it() {
    let mut issues = Issues::default();
    let first = first_tick(&mut issues);
    assert_eq!(issues.filed.len(), 1, "only sonnet tooling rose (100%, 3 boxes a week); opus feature doubled on one box a week, too few to count: {:#?}", first.lines);
    let (title, body) = &issues.filed[0];
    assert_eq!(title, "Cost per closed box rose more than 25% week on week in sonnet tooling");
    assert!(body.starts_with("Category: tooling\n") && body.contains("from $5.20 over 3 closed boxes the week before to $10.40 over 3 this week (+100%)"), "{body}");
    assert!(first.lines.contains(&"rose sonnet tooling: $5.20 (n 3) the week before -> $10.40 (n 3) this week, +100%".to_owned()), "{:#?}", first.lines);

    let mut held = Issues { open: vec![title.clone()], ..Issues::default() };
    let next = tick(&ledger(), &first.cells, &mut held, AT, false);
    assert!(held.filed.is_empty(), "the open issue holds the cell");
    assert!(next.lines.contains(&format!("  held by an issue filed for it: {title}")), "{:#?}", next.lines);
}

#[test]
fn a_dry_run_or_an_unread_tracker_files_nothing() {
    let mut issues = Issues::default();
    let dry = tick(&ledger(), &Cells::new(), &mut issues, AT, true);
    assert!(issues.filed.is_empty());
    assert!(dry.lines.iter().any(|line| line.starts_with("  would file: Cost per closed box rose")), "{:#?}", dry.lines);

    let mut unread = Issues { unreadable: true, ..Issues::default() };
    let blind = tick(&ledger(), &Cells::new(), &mut unread, AT, false);
    assert!(unread.filed.is_empty(), "with no open list it cannot know an issue already holds the cell");
    assert_eq!(blind.lines.last().unwrap(), "open north issues: unread (gh: not logged in)");
    assert!(blind.failed);
}

#[test]
fn an_issue_closed_in_the_last_week_still_holds_its_cell_and_a_refused_filing_fails_the_tick() {
    let title = "Cost per closed box rose more than 25% week on week in sonnet tooling".to_owned();
    let mut closed = Issues { closed: vec![title.clone()], ..Issues::default() };
    let held = first_tick(&mut closed);
    assert!(closed.filed.is_empty() && !held.failed, "{:#?}", held.lines);
    assert!(held.lines.contains(&format!("  held by an issue filed for it: {title}")), "{:#?}", held.lines);

    let mut refusing = Issues { refuses: true, ..Issues::default() };
    let refused = first_tick(&mut refusing);
    assert!(refused.lines.contains(&"  not filed: gh issue create: HTTP 403".to_owned()), "{:#?}", refused.lines);
    assert_eq!(refused.lines.last().unwrap(), "open north issues: 0");
    assert!(refused.failed);
}

#[test]
fn unread_arm_evidence_keeps_the_prior_routing_cells_and_fails_the_tick() {
    let mut ledger = ledger();
    ledger.evidence.clear();
    ledger.notes.push("worker-ledger --recommend \"Category: feature\" unread".into());
    let prior = first_tick(&mut Issues::default()).cells;
    let blind = tick(&ledger, &prior, &mut Issues::default(), AT, false);
    assert_eq!(blind.cells.get("route feature"), Some(&json!("sonnet-high")));
    assert!(blind.records.is_empty(), "a failed read neither drops nor re-adds a route: {:?}", blind.records);
    assert!(blind.lines[1].starts_with("unread: worker-ledger --recommend"), "{:#?}", blind.lines);
    assert!(blind.failed);
}
