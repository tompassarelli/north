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


#[test]
fn a_rise_files_only_while_it_holds_with_the_weeks_dearest_box_dropped() {
    let filed = |outputs: [i64; 3]| {
        let week: Vec<SpendRun> = outputs.iter().enumerate().map(|(index, output)| spent(&format!("north#5{index}"), OPUS, "tooling", "2026-10-24", *output)).collect();
        let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&boxes("40", 3, OPUS, "tooling", "2026-10-17", 2_000_000), "2026-10-13T06:00:00Z"), evidence: BTreeMap::new(), notes: Vec::new(), estimates: Ok(Vec::new()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new(), projections: Ok(Vec::new()) };
        let mut issues = Issues::default();
        let (_, ticked) = two_ticks(&ledger, &mut issues);
        ticked.lines.iter().any(|line| line.starts_with("rose "))
    };
    assert!(!filed([1_000_000, 5_000_000, 9_000_000]), "the median box alone is past the bar, the box beside it is not");
    assert!(filed([5_000_000, 5_000_000, 9_000_000]));
}
