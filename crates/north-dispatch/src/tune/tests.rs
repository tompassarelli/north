use super::costs::Cost;
use super::dividends::{self, Reading};
use super::estimates;
use super::rises::TITLE;
use super::{Ledger, Prior, Tick, Tracker, tick};
use crate::platform::rolling_issue::{self, Breaches};
use crate::router::pricing::PriceTable;
use crate::ledger::{ClosedReport, EstimatedRun, SpendRun, summarize};
use crate::ledger::closed::ArmBox;
use crate::dispatch::{Evidence, Tier};
use crate::threads::calibration::predictions::Brier;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

const AT: &str = "2026-10-27T06:00:00Z";
const SONNET: (&str, &str) = ("sonnet", "claude-sonnet-5-5");
const OPUS: (&str, &str) = ("medium", "claude-opus-5-5");

fn run(item: &str, (tier, model): (&str, &str), category: &str, started: &str, ended: &str) -> SpendRun {
    SpendRun {
        agent: format!("agent-{item}"),
        item: Some(item.into()),
        role: "worker".into(),
        tier: tier.into(),
        category: category.into(),
        arm: String::new(),
        model: Some(model.into()),
        started: started.into(),
        ended: ended.into(),
        landed: true,
        reworked: false,
        input: 3_000_000,
        cache_read: 2_000_000,
        output: 100_000,
        skills: Vec::new(),
    }
}

fn report(runs: &[SpendRun], since: &str) -> ClosedReport {
    let closed: HashSet<String> = runs.iter().filter_map(|run| run.item.clone()).collect();
    summarize(runs, &closed, since, &PriceTable::load().unwrap())
}

/// Sonnet closes two feature boxes at one run each and Opus one at two runs; Sonnet closes three
/// tooling boxes at two runs each, which cost one run each the week before, and Opus's one feature
/// box cost one run the week before.
pub(super) fn ledger() -> Ledger {
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
        ("feature".to_owned(), Evidence { tiers: arms((60.0, 15), (70.0, 15)), arm_runs: stamped(15, 15), ..Evidence::default() }.priced(&arm_boxes("feature", (15, 5.2), (15, 20.8)), "feature")),
        ("tooling".to_owned(), Evidence { tiers: arms((80.0, 4), (0.0, 0)), arm_runs: stamped(4, 0), ..Evidence::default() }.priced(&arm_boxes("tooling", (4, 10.4), (0, 0.0)), "tooling")),
    ]);
    Ledger { week, before, evidence, notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) }
}

#[derive(Default)]
pub(super) struct Issues {
    open: Vec<String>,
    unreadable: bool,
    refuses: bool,
    told: Vec<String>,
    mute: bool,
    rolling: BTreeMap<String, (u64, String)>,
    created: Vec<(String, String)>,
    edited: Vec<String>,
    settled: Vec<String>,
}

impl Tracker for Issues {
    fn open(&self) -> Result<Vec<String>, String> {
        if self.unreadable {
            return Err("gh: not logged in".into());
        }
        Ok(self.open.clone())
    }

    fn tell_proxy(&mut self, text: &str) -> Result<(), String> {
        if self.mute {
            return Err("north fleet send proxy: exit status: 1".into());
        }
        self.told.push(text.to_owned());
        Ok(())
    }

    fn rolling(&mut self) -> &mut dyn rolling_issue::Tracker {
        self
    }
}

impl rolling_issue::Tracker for Issues {
    fn open(&self, title: &str) -> Result<Option<rolling_issue::Issue>, String> {
        if self.unreadable {
            return Err("gh: not logged in".into());
        }
        Ok(self.rolling.get(title).map(|(number, body)| rolling_issue::Issue { number: *number, body: body.clone() }))
    }

    fn body(&self, number: u64) -> Result<String, String> {
        Ok(self.rolling.values().find(|(open, _)| *open == number).map(|(_, body)| body.clone()).unwrap_or_default())
    }

    fn listed(&self, _: &str) -> Result<Vec<(u64, String)>, String> {
        Ok(Vec::new())
    }

    fn create(&mut self, title: &str, body: &str) -> Result<(), String> {
        if self.refuses {
            return Err("gh issue create: HTTP 403".into());
        }
        self.created.push((title.to_owned(), body.to_owned()));
        self.open.push(title.to_owned());
        self.rolling.insert(title.to_owned(), (950 + self.created.len() as u64, body.to_owned()));
        Ok(())
    }

    fn edit(&mut self, number: u64, body: &str) -> Result<(), String> {
        self.edited.push(body.to_owned());
        self.rolling.values_mut().filter(|(open, _)| *open == number).for_each(|(_, kept)| *kept = body.to_owned());
        Ok(())
    }

    fn close(&mut self, number: u64, comment: &str) -> Result<(), String> {
        self.settled.push(comment.to_owned());
        let closed: Vec<String> = self.rolling.iter().filter(|(_, (open, _))| *open == number).map(|(title, _)| title.clone()).collect();
        self.open.retain(|title| !closed.contains(title));
        self.rolling.retain(|title, _| !closed.contains(title));
        Ok(())
    }
}

fn two_ticks(ledger: &Ledger, issues: &mut Issues) -> (Tick, Tick) {
    let first = tick(ledger, &Prior::default(), &[], issues, AT, false);
    let second = tick(ledger, &Prior { cells: first.cells.clone(), breaches: first.breaches.clone(), ..Prior::default() }, &[], issues, AT, false);
    (first, second)
}

fn rolled(ledger: &Ledger) -> String {
    let mut issues = Issues::default();
    let (_, second) = two_ticks(ledger, &mut issues);
    assert_eq!(issues.created.len(), 1, "{:#?}", second.lines);
    assert_eq!(issues.created[0].0, TITLE);
    issues.created.remove(0).1
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
    assert_eq!(tick.lines.last().unwrap(), "open north issues: 2", "the two open; one risen tick files nothing: {:#?}", tick.lines);
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
fn one_risen_tick_posts_nothing_two_in_a_row_open_one_rolling_issue_an_unchanged_tick_posts_nothing_and_a_calm_one_closes_it() {
    let mut issues = Issues::default();
    let first = first_tick(&mut issues);
    assert!(issues.created.is_empty(), "one risen tick posts nothing: {:#?}", first.lines);
    assert!(first.lines.contains(&"rose sonnet tooling: $5.20 (n 3) the week before -> $10.40 (n 3) this week, +100%".to_owned()), "{:#?}", first.lines);

    let next = |previous: &Tick| Prior { cells: previous.cells.clone(), breaches: previous.breaches.clone(), ..Prior::default() };
    let second = tick(&ledger(), &next(&first), &[], &mut issues, AT, false);
    assert_eq!(issues.created.len(), 1, "only sonnet tooling rose (100%, 3 boxes a week); opus feature doubled on one box a week, too few to count: {:#?}", second.lines);
    let (title, body) = &issues.created[0];
    assert_eq!(title, TITLE);
    assert!(body.starts_with("Category: tooling\n") && body.contains("## sonnet tooling\n\nMedian cost of a closed box rose from $5.20 over 3 closed boxes the week before to $10.40 over 3 this week (+100%)"), "{body}");
    assert_eq!(body.matches("\n## ").count(), 2, "the Done when and the one risen cell: {body}");
    assert_eq!(second.lines.last().unwrap(), "open north issues: 1", "{:#?}", second.lines);

    let third = tick(&ledger(), &next(&second), &[], &mut issues, AT, false);
    assert!(issues.created.len() == 1 && issues.edited.is_empty() && issues.settled.is_empty(), "an unchanged tick posts nothing: {:#?}", third.lines);

    let calm = Ledger { week: ledger().before, ..ledger() };
    tick(&calm, &next(&third), &[], &mut issues, AT, false);
    assert_eq!(issues.settled.len(), 1, "the first calm tick closes the rolling issue");
    assert!(issues.rolling.is_empty() && issues.created.len() == 1);
}

#[test]
fn a_dry_run_or_an_unread_tracker_files_nothing() {
    let mut issues = Issues::default();
    let dry = tick(&ledger(), &Prior::default(), &[], &mut issues, AT, true);
    assert!(issues.created.is_empty());
    assert_eq!(dry.breaches, Breaches::default(), "a dry run leaves the gate as it was");
    assert!(dry.lines.contains(&format!("  would file: {TITLE}")), "{:#?}", dry.lines);

    let mut unread = Issues { unreadable: true, ..Issues::default() };
    let (_, blind) = two_ticks(&ledger(), &mut unread);
    assert!(unread.created.is_empty(), "with no open list it cannot know an issue already holds the cell");
    assert!(blind.lines.contains(&format!("  not settled: {TITLE}: gh: not logged in")), "{:#?}", blind.lines);
    assert_eq!(blind.lines.last().unwrap(), "open north issues: unread (gh: not logged in)");
    assert!(blind.failed);
}

#[test]
fn a_refused_rolling_issue_fails_the_tick() {
    let mut refusing = Issues { refuses: true, ..Issues::default() };
    let (first, refused) = two_ticks(&ledger(), &mut refusing);
    assert!(!first.failed, "one risen tick asks nothing of the tracker: {:#?}", first.lines);
    assert!(refused.lines.contains(&format!("  not settled: {TITLE}: gh issue create: HTTP 403")), "{:#?}", refused.lines);
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

/// Closed arm boxes of one category at one spend each: (count, usd) on the Sonnet arm and the Opus arm.
fn arm_boxes(category: &str, sonnet: (usize, f64), opus: (usize, f64)) -> ClosedReport {
    let armed = |arm: &str, (count, usd): (usize, f64)| (0..count).map(|number| ArmBox { item: format!("north#{arm}-{number}"), tier: String::new(), arm: arm.into(), usd, category: category.into(), closed: true, reworked: false }).collect::<Vec<_>>();
    ClosedReport { skipped: Vec::new(), closed: 0, cells: Vec::new(), arms: [armed("sonnet-high", sonnet), armed("worker", opus)].concat(), spend_usd: 0.0, leads_usd: 0.0, ..ClosedReport::default() }
}

fn stamped(sonnet: usize, opus: usize) -> BTreeMap<Tier, usize> {
    BTreeMap::from([(Tier::Sonnet, sonnet), (Tier::Opus, opus)])
}

fn arms(sonnet: (f64, usize), opus: (f64, usize)) -> BTreeMap<String, (f64, usize)> {
    BTreeMap::from([("claude:sonnet-high".to_owned(), sonnet), ("claude:opus-medium".to_owned(), opus)])
}

#[test]
fn sonnet_closing_60_percent_at_3_dollars_a_box_beats_opus_closing_70_percent_at_9_and_its_route_cell_is_written() {
    let runs = [boxes("1", 6, SONNET, "tooling", "2026-10-22", 300_000), boxes("2", 7, OPUS, "tooling", "2026-10-23", 450_000)].concat();
    let week = report(&runs, "2026-10-20T06:00:00Z");
    let evidence = BTreeMap::from([("tooling".to_owned(), Evidence { tiers: arms((60.0, 15), (70.0, 15)), arm_runs: stamped(15, 15), ..Evidence::default() }.priced(&arm_boxes("tooling", (15, 3.0), (15, 9.0)), "tooling"))]);
    let ledger = Ledger { week, before: report(&[], "2026-10-13T06:00:00Z"), evidence, notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
    let tick = tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, false);
    assert_eq!(tick.cells.get("route tooling"), Some(&json!("sonnet-high")), "{:#?}", tick.lines);
    let record = tick.records.iter().find(|record| record["cell"] == "route tooling").unwrap();
    let claim = record["claim"].as_str().unwrap();
    assert!(claim.contains("sonnet-high 60% of 15, 15 of 15 arm runs, 15 of 15 closed arm boxes, $3.00 per closed box") && claim.contains("worker 70% of 15, 15 of 15 arm runs, 15 of 15 closed arm boxes, $9.00 per closed box"), "{record}");
}

#[test]
fn a_cell_30_percent_dearer_than_the_week_before_files_exactly_one_issue_and_one_20_percent_dearer_files_none() {
    let week = [boxes("1", 3, SONNET, "tooling", "2026-10-24", 1_300_000), boxes("2", 3, SONNET, "feature", "2026-10-24", 1_200_000)].concat();
    let before = [boxes("3", 3, SONNET, "tooling", "2026-10-17", 1_000_000), boxes("4", 3, SONNET, "feature", "2026-10-17", 1_000_000)].concat();
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&before, "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
    let body = rolled(&ledger);
    assert_eq!(body.matches("\n## sonnet ").count(), 1, "{body}");
    assert!(body.contains("## sonnet tooling\n\nMedian cost of a closed box rose from $10.00 over 3 closed boxes the week before to $13.00 over 3 this week (+30%)"), "{body}");
    assert!(body.contains("\n- north#10 $13.00: agent-north#10 sonnet tooling from 2026-10-24T10:00:00Z $13.00\n"), "{body}");
}

#[test]
fn a_rise_names_every_run_of_a_box_escalated_off_its_first_tier_so_the_issue_traces_it_without_the_ledger() {
    const HAIKU: (&str, &str) = ("haiku", "claude-haiku-5-5");
    let escalations = (0..2).map(|index| SpendRun { input: 0, cache_read: 0, output: 1_000_000, ..run(&format!("north#1{index}"), SONNET, "tooling", "2026-10-24T11:00:00Z", "2026-10-24T11:30:00Z") });
    let week: Vec<SpendRun> = boxes("1", 3, HAIKU, "mechanical", "2026-10-24", 10_000).into_iter().chain(escalations).collect();
    let before = boxes("3", 3, HAIKU, "mechanical", "2026-10-17", 10_000);
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&before, "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
    let body = rolled(&ledger);
    assert!(body.contains("\n- north#10 $10.01: agent-north#10 haiku mechanical from 2026-10-24T10:00:00Z $0.01; agent-north#10 sonnet tooling from 2026-10-24T11:00:00Z $10.00\n"), "{body}");
}

#[test]
fn a_cell_of_boxes_no_label_has_named_yet_files_no_rise_however_dear_this_week_is() {
    let week = boxes("1", 3, OPUS, "unknown", "2026-10-24", 9_000_000);
    let before = boxes("3", 3, OPUS, "unknown", "2026-10-17", 1_000_000);
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&before, "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
    let mut issues = Issues::default();
    let (_, quiet) = two_ticks(&ledger, &mut issues);
    assert!(issues.created.is_empty(), "{:#?}", quiet.lines);
    assert!(!quiet.lines.iter().any(|line| line.starts_with("rose ")), "{:#?}", quiet.lines);
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
    let (_, quiet) = two_ticks(&Ledger { week, before, evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) }, &mut issues);
    assert!(issues.created.is_empty(), "{:#?}", quiet.lines);
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

    let next = tick(&ledger(), &Prior { cells: first.cells, costs: earlier, ..Prior::default() }, &[], &mut Issues::default(), "2026-10-28T06:00:00Z", false);
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

#[test]
fn one_umbrella_box_with_many_runs_lifts_the_mean_past_the_bar_but_files_no_rise_while_the_median_box_holds() {
    let box_runs = |item: &str, count: usize, day: &str| -> Vec<SpendRun> {
        (0..count).map(|n| run(item, SONNET, "bug-known-cause", &format!("{day}T{:02}:00:00Z", 8 + n), &format!("{day}T{:02}:30:00Z", 8 + n))).collect()
    };
    let before = report(&[box_runs("north#30", 2, "2026-10-15"), box_runs("north#31", 2, "2026-10-16"), box_runs("north#32", 2, "2026-10-17")].concat(), "2026-10-13T06:00:00Z");
    let week = report(&[box_runs("north#33", 2, "2026-10-22"), box_runs("north#34", 2, "2026-10-23"), box_runs("north#35", 2, "2026-10-24"), box_runs("north#36", 12, "2026-10-25")].concat(), "2026-10-20T06:00:00Z");
    let cell = |report: &ClosedReport| report.cells.iter().find(|cell| cell.tier == SONNET.0).map(|cell| (cell.closed, cell.usd_per_closed())).unwrap();
    let ((was_n, was), (now_n, now)) = (cell(&before), cell(&week));
    assert!((was_n, now_n) == (3, 4) && now > was * 1.25, "{was} -> {now}");

    let mut issues = Issues::default();
    let (_, ticked) = two_ticks(&Ledger { week, before, evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) }, &mut issues);
    assert!(issues.created.is_empty(), "{:#?}", ticked.lines);
}

fn estimated(tier: &str, category: &str, (minutes, eta_min): (f64, f64), (usd, estimated_usd): (f64, Option<f64>)) -> EstimatedRun {
    EstimatedRun { tier: tier.into(), category: category.into(), minutes, eta_min, usd, estimated_usd }
}

/// Sonnet tooling's runs land near their ETA with no cost estimate; Opus feature's three runs take
/// 2-3x their ETA and spend 2-3x their expected spend; Haiku research's three runs take 3x theirs;
/// Sonnet research's two runs take 3x theirs.
fn estimated_ledger() -> Ledger {
    let opus = |minutes: f64, usd: f64| estimated("opus-medium", "feature", (minutes, 20.0), (usd, Some(2.0)));
    let slow = |tier: &str, category: &str| estimated(tier, category, (30.0, 10.0), (1.0, None));
    let estimates = vec![
        estimated("sonnet", "tooling", (10.0, 10.0), (1.0, None)),
        estimated("sonnet", "tooling", (12.0, 10.0), (1.0, None)),
        estimated("sonnet", "tooling", (8.0, 10.0), (1.0, None)),
        estimated("sonnet", "tooling", (30.0, 10.0), (1.0, None)),
        opus(40.0, 4.0),
        opus(50.0, 5.0),
        opus(60.0, 6.0),
        slow("haiku", "research"),
        slow("haiku", "research"),
        slow("haiku", "research"),
        slow("sonnet", "research"),
        slow("sonnet", "research"),
    ];
    Ledger { estimates: Ok(estimates), ..ledger() }
}

fn drift_issues(issues: &Issues) -> Vec<&(String, String)> {
    issues.created.iter().filter(|(title, _)| title == estimates::TITLE).collect()
}

#[test]
fn several_drifted_cells_keep_one_rolling_issue_behind_the_two_breach_gate_with_no_category_so_dispatch_never_staffs_it() {
    let mut issues = Issues::default();
    let first = tick(&estimated_ledger(), &Prior::default(), &[], &mut issues, AT, false);
    for line in [
        "estimate haiku research: minutes median 3.00x p90 3.00x inside 0% (n 3); cost n/a",
        "estimate opus-medium feature: minutes median 2.50x p90 3.00x inside 0% (n 3); cost median 2.50x p90 3.00x inside 0% (n 3)",
        "estimate sonnet research: minutes median 3.00x p90 3.00x inside 0% (n 2); cost n/a",
        "estimate sonnet tooling: minutes median 1.00x p90 3.00x inside 50% (n 4); cost n/a",
        "drifted haiku research: minutes median 3.00x outside 0.67-1.5x",
        "drifted opus-medium feature: minutes median 2.50x, cost median 2.50x outside 0.67-1.5x",
    ] {
        assert!(first.lines.contains(&line.to_owned()), "{line}: {:#?}", first.lines);
    }
    assert!(issues.created.is_empty() && !first.failed, "one drifted tick posts nothing: {:#?}", first.lines);

    let next = |previous: &Tick| Prior { cells: previous.cells.clone(), breaches: previous.breaches.clone(), ..Prior::default() };
    let second = tick(&estimated_ledger(), &next(&first), &[], &mut issues, AT, false);
    let drifts = drift_issues(&issues);
    assert_eq!(drifts.len(), 1, "two drifted cells share one issue: {:#?}", second.lines);
    let body = &drifts[0].1;
    assert!(!body.contains("Category:") && !body.contains("Done when"), "dispatch staffs no box from it: {body}");
    assert!(body.contains("## haiku research\n\n- minutes: actual over estimate median 3.00x p90 3.00x inside 0% (n 3)\n"), "{body}");
    assert!(body.contains("## opus-medium feature\n\n- minutes: actual over estimate median 2.50x p90 3.00x inside 0% (n 3)\n- cost: actual over estimate median 2.50x"), "{body}");
    assert_eq!(body.matches("## ").count(), 2, "sonnet research holds 2 runs, too few, and sonnet tooling sits inside the band: {body}");
    assert!(!second.failed, "{:#?}", second.lines);

    let third = tick(&estimated_ledger(), &next(&second), &[], &mut issues, AT, false);
    assert!(drift_issues(&issues).len() == 1 && issues.edited.is_empty() && issues.settled.is_empty(), "an unchanged tick posts nothing: {:#?}", third.lines);

    let calm = Ledger { estimates: Ok(Vec::new()), ..estimated_ledger() };
    tick(&calm, &next(&third), &[], &mut issues, AT, false);
    assert!(!issues.rolling.contains_key(estimates::TITLE) && issues.settled.iter().any(|comment| comment.contains("sits inside 0.67-1.5x")), "the first calm tick closes it: {:?}", issues.settled);

    let unread = tick(&Ledger { estimates: Err("no such table: runs".into()), ..ledger() }, &Prior::default(), &[], &mut Issues::default(), AT, false);
    assert!(unread.failed && unread.lines.contains(&"estimates: unread (no such table: runs)".to_owned()), "{:#?}", unread.lines);
}

fn effort_boxes(category: &str, medium: (usize, usize, f64), high: (usize, usize, f64)) -> ClosedReport {
    let armed = |arm: &str, (count, reworked, usd): (usize, usize, f64)| (0..count).map(|number| ArmBox { item: format!("north#{arm}-{number}"), tier: "opus-medium".into(), arm: arm.into(), usd, category: category.into(), closed: true, reworked: number < reworked }).collect::<Vec<_>>();
    ClosedReport { skipped: Vec::new(), closed: 0, cells: Vec::new(), arms: [armed("effort-medium", medium), armed("effort-high", high)].concat(), spend_usd: 0.0, leads_usd: 0.0, boxes: Vec::new(), loaded: BTreeMap::new() }
}

#[test]
fn a_fixture_ledger_with_both_efforts_writes_the_cheaper_kept_box_effort_cell_and_keeps_it_when_unread() {
    let stamped = BTreeMap::from([("effort-medium".to_owned(), 15), ("effort-high".to_owned(), 15)]);
    let evidence = |medium, high| Evidence::default().counted(&stamped).priced(&effort_boxes("research", medium, high), "research");
    let ledger = Ledger { week: report(&[], "2026-10-20T06:00:00Z"), before: report(&[], "2026-10-13T06:00:00Z"), evidence: BTreeMap::from([("research".to_owned(), evidence((15, 10, 5.0), (15, 0, 8.0)))]), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
    let first = tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, false);
    assert_eq!(first.cells.get("effort research"), Some(&json!("high")), "{:#?}", first.lines);
    let record = first.records.iter().find(|record| record["cell"] == "effort research").unwrap();
    assert_eq!((record["rows"].clone(), record["n"].clone()), (json!(30), json!(30)));
    assert!(record["claim"].as_str().unwrap().contains("effort-medium 15 of 15 arm runs, 15 of 15 closed arm boxes, $15.00 per kept box; effort-high 15 of 15 arm runs, 15 of 15 closed arm boxes, $8.00 per kept box"), "{record}");
    let cheaper = Ledger { evidence: BTreeMap::from([("research".to_owned(), evidence((15, 0, 5.0), (15, 0, 8.0)))]), ..ledger };
    assert_eq!(tick(&cheaper, &Prior::default(), &[], &mut Issues::default(), AT, false).cells.get("effort research"), Some(&json!("medium")));
    let blind = Ledger { evidence: BTreeMap::new(), notes: vec!["arm runs of research unread".into()], ..cheaper };
    let kept = tick(&blind, &Prior { cells: first.cells.clone(), ..Prior::default() }, &[], &mut Issues::default(), AT, false);
    assert_eq!(kept.cells.get("effort research"), Some(&json!("high")), "a failed read keeps the effort cell");
}

/// A finished run carrying every field the coverage table reads, as one launch path records it.
fn recorded_run(path: &str) -> Value {
    json!({
        "model": "claude-opus-5-5", "effort": "medium", "cost_usd": 1.25, "tokens": 40_000, "category": "tooling",
        "spec": "measured", "scope": "module", "surface": "rust", "verify": "local-test", "minutes": 30, "eta_min": 40,
        "eta_source": "tune", "budget_usd": 2.5, "budget_source": "ledger",
        "active_min": 22.5, "turns": 31, "tool_calls": 48, "brief_text": format!("Item: north#255 launched by {path}"), "compactions": 0,
    })
}

fn covered(runs: Vec<Value>) -> Tick {
    covered_from(Ok(runs))
}

#[test]
fn a_run_from_each_launch_path_carrying_every_field_holds_every_field_at_full_coverage() {
    let runs = ["fleet launch", "dispatch", "Agent tool", "cloud"].map(recorded_run).to_vec();
    let tick = covered(runs);
    assert!(tick.lines.contains(&"run field coverage 24h over 4 finished runs, floor 98%: every field holds".to_owned()), "{:#?}", tick.lines);
    for field in ["model", "effort", "cost", "tokens", "category", "facets", "wall minutes", "ETA", "ETA source", "E", "E source", "active minutes", "turns", "tool calls", "brief text", "compactions"] {
        assert!(tick.lines.contains(&format!("  {field} 100% (4 of 4)")), "{field}: {:#?}", tick.lines);
    }
    assert!(!tick.failed);
}

#[test]
fn the_coverage_table_prints_each_fields_share_and_fails_the_tick_with_one_field_at_97_percent() {
    let mut runs: Vec<Value> = (0..100).map(|index| recorded_run(["fleet launch", "dispatch", "Agent tool", "cloud"][index % 4])).collect();
    for run in &mut runs[..3] {
        run["turns"] = Value::Null;
    }
    runs[3]["category"] = json!("unknown");
    runs[4]["brief_text"] = json!("");
    let tick = covered(runs);
    assert!(tick.lines.contains(&"run field coverage 24h over 100 finished runs, floor 98%: under 98%: turns".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  turns 97% (97 of 100)  BELOW".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  category 99% (99 of 100)".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  brief text 99% (99 of 100)".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  model 100% (100 of 100)".to_owned()), "{:#?}", tick.lines);
    assert_eq!(tick.lines.last().unwrap(), "open north issues: 0", "{:#?}", tick.lines);
    assert!(tick.failed);
}

#[test]
fn category_facets_and_eta_are_read_over_launched_work_while_a_reviewer_without_them_is_set_aside_and_a_worker_without_them_still_fails() {
    let reviewer = || {
        let mut run = recorded_run("Agent tool");
        run["role"] = json!("reviewer");
        run["category"] = json!("unknown");
        run["eta_min"] = Value::Null;
        run
    };
    let mut runs: Vec<Value> = (0..98).map(|_| recorded_run("fleet launch")).collect();
    runs.extend([reviewer(), reviewer()]);
    let tick = covered(runs.clone());
    assert!(tick.lines.contains(&"run field coverage 24h over 100 finished runs, floor 98%: every field holds".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  category 100% (98 of 98)".to_owned()), "{:#?}", tick.lines);
    assert!(tick.lines.contains(&"  category, facets and ETA read over launched work: 2 reviewer, judge, probe or proxy runs set aside".to_owned()), "{:#?}", tick.lines);
    assert!(!tick.failed);
    for run in &mut runs[..3] {
        run["role"] = json!("worker");
        run["eta_min"] = Value::Null;
    }
    let short = covered(runs);
    assert!(short.lines.contains(&"  ETA 96.9% (95 of 98)  BELOW".to_owned()), "{:#?}", short.lines);
    assert!(short.failed);
}

#[test]
fn unread_runs_fail_the_tick_and_a_day_without_finished_runs_does_not() {
    let unread = covered_from(Err("threads.db: no such table: runs".into()));
    assert!(unread.lines.contains(&"run field coverage 24h: unread (threads.db: no such table: runs)".to_owned()), "{:#?}", unread.lines);
    assert!(unread.failed);
    let quiet = covered(Vec::new());
    assert!(quiet.lines.contains(&"run field coverage 24h: no finished runs".to_owned()), "{:#?}", quiet.lines);
    assert!(!quiet.failed);
}

fn covered_from(runs: Result<Vec<Value>, String>) -> Tick {
    let ledger = Ledger { runs, ..ledger() };
    tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, false)
}

fn skilled(item: &str, skills: &[&str], landed: bool) -> SpendRun {
    SpendRun { landed, skills: skills.iter().map(|skill| (*skill).to_owned()).collect(), ..spent(item, SONNET, "tooling", "2026-10-22", 100_000) }
}

#[test]
fn the_skill_table_compares_each_skill_with_matched_boxes_and_flags_the_unused_one_and_the_one_that_closes_less() {
    let mut runs: Vec<SpendRun> = (0..6).map(|number| skilled(&format!("north#{}", 20 + number), &[], true)).collect();
    runs.extend((0..4).map(|number| skilled(&format!("north#{}", 30 + number), &["slow-skill"], number < 2)));
    runs.extend((0..3).map(|number| skilled(&format!("north#{}", 40 + number), &["verification"], true)));
    runs.push(SpendRun { role: "proxy".into(), item: None, ..skilled("north#0", &["verification"], false) });
    let closed: HashSet<String> = runs.iter().filter(|run| run.landed).filter_map(|run| run.item.clone()).collect();
    let week = summarize(&runs, &closed, "2026-10-20T06:00:00Z", &PriceTable::load().unwrap());
    let installed = ["slow-skill", "unused-skill", "verification"].map(String::from).to_vec();
    let ledger = Ledger { week, before: report(&[], "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: installed, projections: Ok(Vec::new()) };
    let tick = tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, true);
    let table: Vec<&str> = tick.lines.iter().map(String::as_str).filter(|line| line.contains("skill")).collect();
    assert_eq!(
        table,
        [
            "skills over 14 days: 2 loaded, 3 installed",
            "skill slow-skill: 4 runs, 4 boxes, closed 50% at $2.00/closed vs closed 100% at $1.00/closed on 9 matched boxes",
            "skill verification: 4 runs, 3 boxes, closed 100% at $1.00/closed vs closed 80% at $1.25/closed on 10 matched boxes",
            "unused skill unused-skill: no run loaded it in 14 days",
            "harmful skill slow-skill: closed 50% at $2.00/closed vs closed 100% at $1.00/closed on matched boxes",
        ],
        "verification closes as often at no more cost and is not flagged"
    );
}

/// Thirty Sonnet tooling runs launched with a $20.80 Budget, twice the cell's median $10.40 box:
/// twenty-five spend $10.40 and five $31.20; two Sonnet feature runs carry a $10.40 Budget.
fn budgeted_ledger() -> Ledger {
    let tooling = |usd: f64| estimated("sonnet", "tooling", (10.0, 10.0), (usd, Some(20.8)));
    let mut estimates: Vec<EstimatedRun> = (0..25).map(|_| tooling(10.4)).chain((0..5).map(|_| tooling(31.2))).collect();
    estimates.extend([estimated("sonnet", "feature", (10.0, 10.0), (12.0, Some(10.4))), estimated("sonnet", "feature", (10.0, 10.0), (8.0, Some(10.4)))]);
    estimates.push(estimated("sonnet", "feature", (10.0, 10.0), (50.0, None)));
    Ledger { estimates: Ok(estimates), ..ledger() }
}

#[test]
fn tune_scores_each_category_against_its_stored_budgets_and_reads_the_multiplier_from_the_p90_once_n_reaches_30() {
    let ticked = tick(&budgeted_ledger(), &Prior::default(), &[], &mut Issues::default(), AT, false);
    for line in [
        "budgets 7d: n 32 runs carrying a stored Budget; 1 of 2 categories at n >= 30",
        "budget feature: n 2, over Budget 50%, actual/E p50 0.77x p90 1.15x, multiplier 2x (constant: n under 30)",
        "budget tooling: n 30, over Budget 17%, actual/E p50 0.50x p90 1.50x, multiplier 3x (from p90)",
    ] {
        assert!(ticked.lines.contains(&line.to_owned()), "{line}: {:#?}", ticked.lines);
    }
    assert_eq!(ticked.cells.get("budget_usd sonnet tooling"), Some(&json!(31.2)), "3x the $10.40 median box: the p90 1.5x of Budgets set at 2x");
    assert_eq!(ticked.cells.get("budget_usd sonnet feature"), Some(&json!(10.4)), "under 30 runs the constant 2x holds");
    let record = ticked.records.iter().find(|record| record["cell"] == "budget_usd sonnet tooling").unwrap();
    assert!(record["claim"].as_str().unwrap().starts_with("3x (p90 1.50x of actual/E over n=30 runs carrying a Budget, set at 2.00x) the median $10.40"), "{record}");

    let unbudgeted = tick(&ledger(), &Prior::default(), &[], &mut Issues::default(), AT, false);
    assert!(unbudgeted.lines.contains(&"budgets 7d: n 0 runs carrying a stored Budget; 0 of 0 categories at n >= 30".to_owned()), "{:#?}", unbudgeted.lines);
    assert_eq!(unbudgeted.cells.get("budget_usd sonnet tooling"), Some(&json!(20.8)));
}

#[test]
fn tune_prints_each_categorys_tier_success_brier_against_its_base_rate_and_fails_the_tick_when_unread() {
    let briers = vec![
        Brier { category: "feature".into(), n: 1_828, predicted: 0.271, base: 0.248 },
        Brier { category: "tooling".into(), n: 2, predicted: 0.185, base: 0.347_222 },
    ];
    let scored = tick(&Ledger { briers: Ok(briers), ..ledger() }, &Prior::default(), &[], &mut Issues::default(), AT, true);
    for line in ["tier success feature: Brier 0.271 against base-rate Brier 0.248 (n 1828, 7d)", "tier success tooling: Brier 0.185 against base-rate Brier 0.347 (n 2, 7d)"] {
        assert!(scored.lines.contains(&line.to_owned()), "{line}: {:#?}", scored.lines);
    }
    assert!(!scored.failed, "{:#?}", scored.lines);

    let none = tick(&ledger(), &Prior::default(), &[], &mut Issues::default(), AT, true);
    assert!(none.lines.contains(&"tier success: no first run in 7d carries a stored prediction".to_owned()), "{:#?}", none.lines);

    let unread = tick(&Ledger { briers: Err("no such table: runs".into()), ..ledger() }, &Prior::default(), &[], &mut Issues::default(), AT, true);
    assert!(unread.failed && unread.lines.contains(&"tier success: unread (no such table: runs)".to_owned()), "{:#?}", unread.lines);
}
