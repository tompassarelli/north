use super::{Ledger, Prior, Tick, Tracker, tick};
use crate::platform::rolling_issue;
use crate::router::pricing::PriceTable;
use crate::ledger::{ClosedReport, SpendRun, summarize};
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
        box_text: None,
        model: Some(model.into()),
        effort: String::new(),
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
    summarize(runs, &closed, &Default::default(), since, &PriceTable::load().unwrap())
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
    fn open(&self) -> Result<usize, String> {
        if self.unreadable {
            return Err("gh: not logged in".into());
        }
        Ok(self.open.len())
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

fn spent(item: &str, (tier, model): (&str, &str), category: &str, day: &str, output: i64) -> SpendRun {
    SpendRun { input: 0, cache_read: 0, output, ..run(item, (tier, model), category, &format!("{day}T10:00:00Z"), &format!("{day}T10:30:00Z")) }
}

fn boxes(prefix: &str, count: usize, arm: (&str, &str), category: &str, day: &str, output: i64) -> Vec<SpendRun> {
    (0..count).map(|index| spent(&format!("north#{prefix}{index}"), arm, category, day, output)).collect()
}

#[test]
fn a_cell_of_boxes_no_label_has_named_yet_files_no_rise_however_dear_this_week_is() {
    let week = boxes("1", 3, OPUS, "unknown", "2026-10-24", 9_000_000);
    let before = boxes("3", 3, OPUS, "unknown", "2026-10-17", 1_000_000);
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&before, "2026-10-13T06:00:00Z"), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
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
    assert!((was_n, now_n, was_staffed, now_staffed) == (3, 3, true, false), "{was} -> {now}");

    let mut issues = Issues::default();
    let (_, quiet) = two_ticks(&Ledger { week, before, estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() }, &mut issues);
    assert!(issues.created.is_empty(), "{:#?}", quiet.lines);
    assert!(!quiet.lines.iter().any(|line| line.starts_with("rose ")), "{:#?}", quiet.lines);
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
    let (_, ticked) = two_ticks(&Ledger { week, before, estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() }, &mut issues);
    assert!(issues.created.is_empty(), "{:#?}", ticked.lines);
}


#[test]
fn a_rise_files_only_while_it_holds_with_the_weeks_dearest_box_dropped() {
    let filed = |outputs: [i64; 3]| {
        let week: Vec<SpendRun> = outputs.iter().enumerate().map(|(index, output)| spent(&format!("north#5{index}"), OPUS, "tooling", "2026-10-24", *output)).collect();
        let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&boxes("40", 3, OPUS, "tooling", "2026-10-17", 2_000_000), "2026-10-13T06:00:00Z"), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
        let mut issues = Issues::default();
        let (_, ticked) = two_ticks(&ledger, &mut issues);
        ticked.lines.iter().any(|line| line.starts_with("rose "))
    };
    assert!(!filed([1_000_000, 5_000_000, 9_000_000]), "the median box alone is past the bar, the box beside it is not");
    assert!(filed([5_000_000, 5_000_000, 9_000_000]));
}

#[test]
fn a_tick_moves_an_eta_or_budget_cell_at_most_twofold_from_the_prior_tick_however_far_the_week_lies() {
    let week = boxes("70", 3, SONNET, "tooling", "2026-10-24", 9_000_000);
    let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&[], "2026-10-13T06:00:00Z"), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
    let unlimited = tick(&ledger, &Prior::default(), &[], &mut Issues::default(), AT, false);
    let (budget, eta) = ("budget_usd sonnet tooling", "eta_minutes sonnet tooling");
    let free = |cell: &str| unlimited.cells[cell].as_f64().unwrap();
    assert!(free(budget) > 7.14 && free(eta) > 20.0, "{} {}", free(budget), free(eta));

    let prior = Prior { cells: [(budget.to_owned(), serde_json::json!(3.57)), (eta.to_owned(), serde_json::json!(10.0))].into(), ..Prior::default() };
    let held = tick(&ledger, &prior, &[], &mut Issues::default(), AT, false);
    assert_eq!(held.cells[budget], serde_json::json!(7.14));
    assert_eq!(held.cells[eta], serde_json::json!(20.0));
    assert!(held.lines.iter().any(|line| line.starts_with(&format!("changed {budget}: 3.57 -> 7.14")) && line.contains("step limit")), "{:#?}", held.lines);

    let settled = Prior { cells: held.cells, ..Prior::default() };
    let next = tick(&ledger, &settled, &[], &mut Issues::default(), AT, false);
    assert!(next.cells[budget].as_f64().unwrap() > 7.14, "the next tick walks on toward the figure");
}

#[test]
fn a_box_whose_first_run_is_haiku_charges_its_opus_runs_to_the_opus_cell() {
    let haiku = ("haiku", "claude-haiku-5-5");
    let mut first = run("north#60", haiku, "mechanical", "2026-10-24T08:00:00Z", "2026-10-24T08:10:00Z");
    first.landed = false;
    let later = run("north#60", OPUS, "mechanical", "2026-10-24T09:00:00Z", "2026-10-24T09:30:00Z");
    let week = report(&[first.clone(), later.clone()], "2026-10-20T06:00:00Z");
    let spend = |tier: &str| week.cells.iter().find(|cell| cell.tier.contains(tier)).map(|cell| (cell.closed, cell.usd)).unwrap();
    let prices = PriceTable::load().unwrap();
    let price = |run: &SpendRun| prices.run_usd(run.model.as_deref(), &run.tier, run.input, run.cache_read, run.output);
    assert_eq!(spend("opus"), (1, price(&later)), "the Opus run's cost is in the Opus cell");
    assert_eq!(spend("haiku"), (1, price(&first)), "the haiku cell holds only the haiku run's cost");
    assert_eq!(week.closed, 1, "one box closed, not one per cell");
}

const RECORDED_OPUS_HIGH_DEBUGGING: [(f64, f64); 20] = [
    (33.0, 74.0), (265.0, 69.0), (255.0, 69.0), (28.0, 74.0), (73.0, 74.0), (97.0, 69.0), (22.0, 72.0), (20.0, 72.0), (13.0, 72.0), (40.0, 69.0),
    (42.0, 69.0), (9.0, 69.0), (79.0, 69.0), (56.0, 69.0), (84.0, 69.0), (21.0, 69.0), (10.0, 69.0), (26.0, 69.0), (246.0, 68.0), (5.0, 68.0),
];

fn estimated(pairs: &[(f64, f64)]) -> Vec<crate::ledger::EstimatedRun> {
    let at = |(minutes, eta_min): &(f64, f64)| crate::ledger::EstimatedRun { tier: "opus-high".into(), category: "debugging-unknown-cause".into(), minutes: *minutes, eta_min: *eta_min, usd: 1.0, estimated_usd: None };
    pairs.iter().map(at).collect()
}

#[test]
fn an_eta_that_covers_the_quantile_it_targets_is_calibrated_though_the_median_run_sits_far_below_it() {
    let cell = &super::estimates::calibrations(&estimated(&RECORDED_OPUS_HIGH_DEBUGGING))[0];
    let (low, high) = super::estimates::CALIBRATED;
    assert!(!(low..=high).contains(&cell.minutes.median), "the median ratio {} is the reading that filed the cell as drifted", cell.minutes.median);
    assert!(!cell.drifted(), "the ETA covers its p{} of the recorded runs at {}x, so it holds", cell.minutes.percent, cell.minutes.covered);
}

#[test]
fn an_eta_a_quarter_of_the_runs_that_cover_it_would_need_still_drifts() {
    let short: Vec<(f64, f64)> = RECORDED_OPUS_HIGH_DEBUGGING.iter().map(|(minutes, eta)| (*minutes, eta / 4.0)).collect();
    assert!(super::estimates::calibrations(&estimated(&short))[0].drifted());
}

#[test]
fn the_estimate_drift_body_is_filed_through_the_path_north_tune_files_it_by() {
    let drifted = estimated(&[(60.0, 10.0), (70.0, 10.0), (80.0, 10.0)]);
    let ledger = Ledger { week: report(&[], "2026-10-20T06:00:00Z"), before: report(&[], "2026-10-13T06:00:00Z"), estimates: Ok(drifted), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
    let mut issues = Issues::default();
    let (_, ticked) = two_ticks(&ledger, &mut issues);
    assert!(!ticked.lines.iter().any(|line| line.contains("not settled")), "{:#?}", ticked.lines);
    let filed = issues.created.iter().find(|(title, _)| title == super::estimates::TITLE).unwrap_or_else(|| panic!("{:#?}", ticked.lines));
    assert!(filed.1.contains("Category: "), "{}", filed.1);
}
