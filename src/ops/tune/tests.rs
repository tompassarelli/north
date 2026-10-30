use super::costs::Cost;
use super::dividends::{self, Reading};
use super::{Ledger, Prior, Tick, Tracker, tick};
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
        arm: String::new(),
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
    told: Vec<String>,
    mute: bool,
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

    fn tell_proxy(&mut self, text: &str) -> Result<(), String> {
        if self.mute {
            return Err("north fleet send proxy: exit status: 1".into());
        }
        self.told.push(text.to_owned());
        Ok(())
    }
}

fn first_tick(issues: &mut Issues) -> Tick {
    tick(&ledger(), &Prior::default(), &[], issues, AT, false)
}

#[test]
fn the_tick_prints_the_dashboard_line_first_and_the_open_north_issue_count_last() {
    let ledger = ledger();
    let mut issues = Issues { open: vec!["a".into(), "b".into()], ..Issues::default() };
    let tick = tick(&ledger, &Prior::default(), &[], &mut issues, AT, false);
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

    let again = tick(&ledger(), &Prior { cells: first.cells.clone(), ..Prior::default() }, &[], &mut issues, AT, false);
    assert!(again.records.is_empty(), "{:?}", again.records);
    assert!(again.lines.contains(&"no cell changed (7 cells)".to_owned()), "{:#?}", again.lines);

    let mut prior = Prior { cells: first.cells.clone(), ..Prior::default() };
    prior.cells.insert("budget_usd sonnet feature".into(), json!(8.0));
    prior.cells.insert("route tooling".into(), json!("worker"));
    let moved = tick(&ledger(), &prior, &[], &mut issues, AT, false);
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
    let next = tick(&ledger(), &Prior { cells: first.cells.clone(), ..Prior::default() }, &[], &mut held, AT, false);
    assert!(held.filed.is_empty(), "the open issue holds the cell");
    assert!(next.lines.contains(&format!("  held by an issue filed for it: {title}")), "{:#?}", next.lines);
}

#[test]
fn a_dry_run_or_an_unread_tracker_files_nothing() {
    let mut issues = Issues::default();
    let dry = tick(&ledger(), &Prior::default(), &[], &mut issues, AT, true);
    assert!(issues.filed.is_empty());
    assert!(dry.lines.iter().any(|line| line.starts_with("  would file: Cost per closed box rose")), "{:#?}", dry.lines);

    let mut unread = Issues { unreadable: true, ..Issues::default() };
    let blind = tick(&ledger(), &Prior::default(), &[], &mut unread, AT, false);
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
    ledger.notes.push("north worker-ledger --recommend \"Category: feature\" unread".into());
    let prior = Prior { cells: first_tick(&mut Issues::default()).cells, ..Prior::default() };
    let blind = tick(&ledger, &prior, &[], &mut Issues::default(), AT, false);
    assert_eq!(blind.cells.get("route feature"), Some(&json!("sonnet-high")));
    assert!(blind.records.is_empty(), "a failed read neither drops nor re-adds a route: {:?}", blind.records);
    assert!(blind.lines[1].starts_with("unread: north worker-ledger --recommend"), "{:#?}", blind.lines);
    assert!(blind.failed);
}

fn spent(item: &str, (tier, model): (&str, &str), category: &str, day: &str, output: i64) -> SpendRun {
    SpendRun { input: 0, cache_read: 0, output, ..run(item, (tier, model), category, &format!("{day}T10:00:00Z"), &format!("{day}T10:30:00Z")) }
}

fn boxes(prefix: &str, count: usize, arm: (&str, &str), category: &str, day: &str, output: i64) -> Vec<SpendRun> {
    (0..count).map(|index| spent(&format!("north#{prefix}{index}"), arm, category, day, output)).collect()
}

fn arms(sonnet: (f64, usize), opus: (f64, usize)) -> BTreeMap<String, (f64, usize)> {
    BTreeMap::from([("claude:sonnet-high".to_owned(), sonnet), ("claude:opus-medium".to_owned(), opus)])
}

#[test]
fn sonnet_closing_60_percent_at_3_dollars_a_box_beats_opus_closing_70_percent_at_9_and_its_route_cell_is_written() {
    let runs = [boxes("1", 6, SONNET, "tooling", "2026-10-22", 300_000), boxes("2", 7, OPUS, "tooling", "2026-10-23", 450_000)].concat();
    let week = report(&runs, "2026-10-20T06:00:00Z");
    let evidence = BTreeMap::from([("tooling".to_owned(), Evidence { tiers: arms((60.0, 15), (70.0, 15)), usd_per_closed: None }.priced(&week, "tooling"))]);
    let ledger = Ledger { week, before: report(&[], "2026-10-13T06:00:00Z"), evidence, notes: Vec::new() };
    let tick = tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, false);
    assert_eq!(tick.cells.get("route tooling"), Some(&json!("sonnet-high")), "{:#?}", tick.lines);
    let record = tick.records.iter().find(|record| record["cell"] == "route tooling").unwrap();
    let claim = record["claim"].as_str().unwrap();
    assert!(claim.contains("sonnet-high 60% of 15, $3.00 per closed box") && claim.contains("worker 70% of 15, $9.00 per closed box"), "{record}");
}

#[test]
fn a_cell_30_percent_dearer_than_the_week_before_files_exactly_one_issue_and_one_20_percent_dearer_files_none() {
    let week = [boxes("1", 3, SONNET, "tooling", "2026-10-24", 1_300_000), boxes("2", 3, SONNET, "feature", "2026-10-24", 1_200_000)].concat();
    let before = [boxes("3", 3, SONNET, "tooling", "2026-10-17", 1_000_000), boxes("4", 3, SONNET, "feature", "2026-10-17", 1_000_000)].concat();
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&before, "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new() };
    let mut issues = Issues::default();
    let tick = tick(&ledger, &Prior::default(), &[], &mut issues, AT, false);
    assert_eq!(issues.filed.len(), 1, "{:#?}", tick.lines);
    assert_eq!(issues.filed[0].0, "Cost per closed box rose more than 25% week on week in sonnet tooling");
    assert!(issues.filed[0].1.contains("from $10.00 over 3 closed boxes the week before to $13.00 over 3 this week (+30%)"), "{}", issues.filed[0].1);
}

#[test]
fn a_tier_that_staffed_no_run_this_week_files_no_rise_for_the_boxes_it_started_earlier() {
    const SOL: (&str, &str) = ("gpt-6.1-sol high", "gpt-6.1-sol");
    let week = report(
        &[
            run("north#20", SOL, "bug-known-cause", "2026-10-14T10:00:00Z", "2026-10-14T10:30:00Z"),
            run("north#20", OPUS, "bug-known-cause", "2026-10-21T10:00:00Z", "2026-10-21T11:00:00Z"),
            run("north#21", SOL, "bug-known-cause", "2026-10-14T12:00:00Z", "2026-10-14T12:30:00Z"),
            run("north#21", OPUS, "bug-known-cause", "2026-10-22T10:00:00Z", "2026-10-22T11:00:00Z"),
            run("north#22", SOL, "bug-known-cause", "2026-10-15T10:00:00Z", "2026-10-15T10:30:00Z"),
            run("north#22", OPUS, "bug-known-cause", "2026-10-23T10:00:00Z", "2026-10-23T11:00:00Z"),
        ],
        "2026-10-20T06:00:00Z",
    );
    let before = report(
        &[
            run("north#23", SOL, "bug-known-cause", "2026-10-14T10:00:00Z", "2026-10-14T10:30:00Z"),
            run("north#24", SOL, "bug-known-cause", "2026-10-15T10:00:00Z", "2026-10-15T10:30:00Z"),
            run("north#25", SOL, "bug-known-cause", "2026-10-16T10:00:00Z", "2026-10-16T10:30:00Z"),
        ],
        "2026-10-13T06:00:00Z",
    );
    let cell = |report: &ClosedReport| report.cells.iter().find(|cell| cell.tier == SOL.0).map(|cell| (cell.closed, cell.usd_per_closed(), cell.staffed)).unwrap();
    let ((was_n, was, was_staffed), (now_n, now, now_staffed)) = (cell(&before), cell(&week));
    assert!((was_n, now_n, was_staffed, now_staffed) == (3, 3, true, false) && now > was * 1.25, "{was} -> {now}");

    let mut issues = Issues::default();
    let quiet = tick(&Ledger { week, before, evidence: BTreeMap::new(), notes: Vec::new() }, &Prior::default(), &[], &mut issues, AT, false);
    assert!(issues.filed.is_empty(), "{:#?}", quiet.lines);
    assert!(!quiet.lines.iter().any(|line| line.starts_with("rose ")), "{:#?}", quiet.lines);
}

#[test]
fn the_next_days_cost_per_closed_box_prints_beside_the_previous_days_and_a_rerun_the_same_day_adds_no_row() {
    let first = first_tick(&mut Issues::default());
    assert!(first.costs.iter().any(|row| row["cell"] == "sonnet tooling" && row["day"] == "2026-10-27"), "{:?}", first.costs);
    assert!(!first.lines.iter().any(|line| line.starts_with("cost per closed box ")), "{:#?}", first.lines);

    let earlier: Vec<Cost> = first.costs.iter().filter_map(Cost::read).collect();
    let rerun = tick(&ledger(), &Prior { cells: first.cells.clone(), costs: earlier.clone(), ..Prior::default() }, &[], &mut Issues::default(), AT, false);
    assert!(rerun.costs.is_empty(), "{:?}", rerun.costs);
    assert!(!rerun.lines.iter().any(|line| line.starts_with("cost per closed box ")), "a day is not compared with itself: {:#?}", rerun.lines);

    let next = tick(&ledger(), &Prior { cells: first.cells.clone(), costs: earlier, ..Prior::default() }, &[], &mut Issues::default(), "2026-10-28T06:00:00Z", false);
    assert!(next.lines.contains(&"cost per closed box sonnet tooling: $10.40 (n 3) today, $10.40 (n 3) on 2026-10-27".to_owned()), "{:#?}", next.lines);
}

const DIVIDENDS: &str = r#"
[[dividend]]
name = "cost per closed box"
issue = "north#112"
metric = "usd_per_closed"
at_most = 12.0

[[dividend]]
name = "Sonnet share of feature and tooling spend"
issue = "north#242"
metric = "family_share"
family = "sonnet"
categories = ["feature", "tooling"]
at_least = 0.9
"#;

fn readings(rows: &[Value]) -> Vec<Reading> {
    rows.iter().filter_map(Reading::read).collect()
}

#[test]
fn a_dividend_past_its_bar_prints_decayed_and_tells_the_proxy_once_until_it_pays_again() {
    let dividends = dividends::parse(DIVIDENDS).unwrap();
    let mut issues = Issues::default();
    let first = tick(&ledger(), &Prior::default(), &dividends, &mut issues, AT, false);
    assert!(first.lines.contains(&"dividend north#112 cost per closed box: $10.40 (at most $12.00), PAYING".to_owned()), "{:#?}", first.lines);
    assert!(first.lines.contains(&"dividend north#242 Sonnet share of feature and tooling spend: 67% (at least 90%), DECAYED".to_owned()), "{:#?}", first.lines);
    assert_eq!(first.readings.len(), 2, "{:?}", first.readings);
    assert_eq!(issues.told.len(), 1, "{:?}", issues.told);
    assert!(issues.told[0].starts_with("dividend DECAYED: Sonnet share of feature and tooling spend (north#242) reads 67% over 6 closed boxes in the 7d ledger, at least 90% wanted; it was first reading."), "{}", issues.told[0]);

    let prior = Prior { readings: readings(&first.readings), ..Prior::default() };
    let rerun = tick(&ledger(), &prior, &dividends, &mut issues, AT, false);
    assert!(rerun.readings.is_empty() && issues.told.len() == 1, "a rerun the same day neither re-records nor re-tells: {:?} {:?}", rerun.readings, issues.told);

    let next = tick(&ledger(), &prior, &dividends, &mut issues, "2026-10-28T06:00:00Z", false);
    assert_eq!(next.readings.len(), 2);
    assert_eq!(issues.told.len(), 1, "still decayed the next day: no second message");

    let mut recovering = dividends.clone();
    recovering[1].bar = dividends::Bar::AtLeast(0.5);
    let paid = tick(&ledger(), &Prior { readings: readings(&next.readings), ..Prior::default() }, &recovering, &mut issues, "2026-10-29T06:00:00Z", false);
    assert!(paid.lines.iter().any(|line| line.ends_with("PAYING")) && issues.told.len() == 1);
    let again = tick(&ledger(), &Prior { readings: readings(&paid.readings), ..Prior::default() }, &dividends, &mut issues, "2026-10-30T06:00:00Z", false);
    assert_eq!(issues.told.len(), 2, "decaying again after paying tells again: {:#?}", again.lines);
    assert!(issues.told[1].contains("it was PAYING on 2026-10-29"), "{}", issues.told[1]);
}

#[test]
fn a_decay_the_proxy_was_not_told_of_keeps_no_row_and_fails_the_tick_and_a_dry_run_tells_no_one() {
    let dividends = dividends::parse(DIVIDENDS).unwrap();
    let mut mute = Issues { mute: true, ..Issues::default() };
    let missed = tick(&ledger(), &Prior::default(), &dividends, &mut mute, AT, false);
    assert!(missed.failed && missed.readings.len() == 1, "only the paying dividend keeps its row: {:?}", missed.readings);
    assert!(missed.lines.iter().any(|line| line.starts_with("  proxy not told of Sonnet share of feature and tooling spend:")), "{:#?}", missed.lines);

    let mut issues = Issues::default();
    let dry = tick(&ledger(), &Prior::default(), &dividends, &mut issues, AT, true);
    assert!(issues.told.is_empty() && !dry.failed, "{:?}", issues.told);
    assert!(dry.lines.iter().any(|line| line.starts_with("  would tell the proxy: dividend DECAYED:")), "{:#?}", dry.lines);
}

#[test]
fn the_dividends_on_main_parse_and_each_names_one_bar() {
    let dividends = dividends::definitions().unwrap();
    assert!(dividends.len() >= 3 && dividends.iter().all(|dividend| dividend.issue.starts_with("north#")), "{dividends:?}");
    assert!(dividends::parse("[[dividend]]\nname = \"x\"\nissue = \"north#1\"\nmetric = \"usd_per_closed\"\n").is_err());
}
