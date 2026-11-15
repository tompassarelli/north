use super::{Ledger, Prior, Tick, Tracker, tell_decays, tick};
use super::dividends::{self, Bar, Reading};
use super::estimates::EstimatedRuns;
use north_pace::router::pricing::PriceTable;
use north_providers::ledger::{ClosedReport, SpendRun, summarize};
use std::collections::HashSet;

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
    told: Vec<String>,
    mute: bool,
}

impl Tracker for Issues {
    fn open(&self) -> Result<usize, String> {
        if self.unreadable {
            return Err("gh: not logged in".into());
        }
        Ok(self.open.len())
    }

    fn tell_coordinator(&mut self, text: &str) -> Result<(), String> {
        if self.mute {
            return Err("north fleet send coordinator: exit status: 1".into());
        }
        self.told.push(text.to_owned());
        Ok(())
    }

}

fn two_ticks(ledger: &Ledger, issues: &mut Issues) -> (Tick, Tick) {
    let first = tick(ledger, &Prior::default(), &[], issues, AT, false);
    let second = tick(ledger, &Prior { cells: first.cells.clone(), alarms: first.alarms.clone(), ..Prior::default() }, &[], issues, AT, false);
    (first, second)
}

fn spent(item: &str, (tier, model): (&str, &str), category: &str, day: &str, output: i64) -> SpendRun {
    SpendRun { input: 0, cache_read: 0, output, ..run(item, (tier, model), category, &format!("{day}T10:00:00Z"), &format!("{day}T10:30:00Z")) }
}

fn boxes(prefix: &str, count: usize, arm: (&str, &str), category: &str, day: &str, output: i64) -> Vec<SpendRun> {
    (0..count).map(|index| spent(&format!("north#{prefix}{index}"), arm, category, day, output)).collect()
}

#[test]
fn an_unlabelled_cell_and_a_tier_that_staffed_no_run_file_no_rise_however_dear_this_week_is() {
    let unknown_week = boxes("1", 3, OPUS, "unknown", "2026-10-24", 9_000_000);
    let unknown_before = boxes("3", 3, OPUS, "unknown", "2026-10-17", 1_000_000);
    let unknown = Ledger { week: report(&unknown_week, "2026-10-20T06:00:00Z"), before: report(&unknown_before, "2026-10-13T06:00:00Z"), estimates: Ok(EstimatedRuns::default()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
    let mut issues = Issues::default();
    let (_, unlabelled) = two_ticks(&unknown, &mut issues);
    assert!(issues.told.is_empty(), "{:#?}", unlabelled.lines);
    assert!(!unlabelled.lines.iter().any(|line| line.starts_with("rose ")), "a cell no label names yet files nothing however dear the week is: {:#?}", unlabelled.lines);

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
    let (_, quiet) = two_ticks(&Ledger { week, before, estimates: Ok(EstimatedRuns::default()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() }, &mut issues);
    assert!(issues.told.is_empty(), "{:#?}", quiet.lines);
    assert!(!quiet.lines.iter().any(|line| line.starts_with("rose ")), "{:#?}", quiet.lines);
}

#[test]
fn one_umbrella_box_with_many_runs_lifts_the_mean_past_the_bar_but_files_no_rise_while_the_median_box_holds() {
    let box_runs = |item: &str, count: usize, day: &str| -> Vec<SpendRun> {
        (0..count).map(|n| run(item, SONNET, "bug-known-cause", &format!("{day}T{:02}:00:00Z", 8 + n), &format!("{day}T{:02}:30:00Z", 8 + n))).collect()
    };
    let before = report(&[box_runs("north#30", 2, "2026-10-15"), box_runs("north#31", 2, "2026-10-16"), box_runs("north#32", 2, "2026-10-17")].concat(), "2026-10-13T06:00:00Z");
    let week_runs = [box_runs("north#33", 2, "2026-10-22"), box_runs("north#34", 2, "2026-10-23"), box_runs("north#35", 2, "2026-10-24"), box_runs("north#36", 12, "2026-10-25")].concat();
    let closed: HashSet<String> = week_runs.iter().filter_map(|run| run.item.clone()).collect();
    let bodies = [("north#36".to_owned(), "- [ ] First piece\n- [ ] Second piece\n".to_owned())].into();
    let week = summarize(&week_runs, &closed, &bodies, "2026-10-20T06:00:00Z", &PriceTable::load().unwrap());
    let cell = |report: &ClosedReport| report.cells.iter().find(|cell| cell.tier == SONNET.0).map(|cell| (cell.closed, cell.usd_per_closed())).unwrap();
    let ((was_n, was), (now_n, now)) = (cell(&before), cell(&week));
    assert!((was_n, now_n) == (3, 4) && now > was * 1.25, "{was} -> {now}");
    let sonnet = week.cells.iter().find(|cell| cell.tier == SONNET.0).unwrap();
    assert_eq!(sonnet.unattributed(), 1, "the umbrella's unslugged runs stay a closed box and leave the median");
    assert_eq!(sonnet.median_box_usd(), report(&box_runs("north#33", 2, "2026-10-22"), "2026-10-20T06:00:00Z").cells[0].median_box_usd(), "the median is the ordinary boxes'");

    let mut issues = Issues::default();
    let (_, ticked) = two_ticks(&Ledger { week, before, estimates: Ok(EstimatedRuns::default()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() }, &mut issues);
    assert!(issues.told.is_empty(), "{:#?}", ticked.lines);
}

#[test]
fn a_rise_is_told_once_and_only_while_it_holds_with_the_weeks_dearest_box_dropped() {
    let risen = |outputs: [i64; 3]| {
        let week: Vec<SpendRun> = outputs.iter().enumerate().map(|(index, output)| spent(&format!("north#5{index}"), OPUS, "tooling", "2026-10-24", *output)).collect();
        let ledger = Ledger { week: report(&week, "2026-10-20T06:00:00Z"), before: report(&boxes("40", 3, OPUS, "tooling", "2026-10-17", 2_000_000), "2026-10-13T06:00:00Z"), estimates: Ok(EstimatedRuns::default()), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
        let mut issues = Issues::default();
        let (_, ticked) = two_ticks(&ledger, &mut issues);
        (ticked.lines.iter().any(|line| line.starts_with("rose ")), issues.told)
    };
    assert_eq!(risen([1_000_000, 5_000_000, 9_000_000]), (false, Vec::new()), "the median box alone is past the bar, the box beside it is not");
    assert_eq!(risen([5_000_000, 5_000_000, 9_000_000]), (true, vec![format!("north tune: {}: opus-medium tooling; `north tune --dry-run` shows them.", north_providers::ledger::rises::TITLE)]), "told on the first risen tick and not again while the cells hold");
}

const RECORDED_OPUS_HIGH_DEBUGGING: [(f64, f64); 20] = [
    (33.0, 74.0), (265.0, 69.0), (255.0, 69.0), (28.0, 74.0), (73.0, 74.0), (97.0, 69.0), (22.0, 72.0), (20.0, 72.0), (13.0, 72.0), (40.0, 69.0),
    (42.0, 69.0), (9.0, 69.0), (79.0, 69.0), (56.0, 69.0), (84.0, 69.0), (21.0, 69.0), (10.0, 69.0), (26.0, 69.0), (246.0, 68.0), (5.0, 68.0),
];

const DAY: i64 = 86_400;

fn estimated(pairs: &[(f64, f64)], ended: i64) -> Vec<north_providers::ledger::EstimatedRun> {
    let at = |(minutes, eta_min): &(f64, f64)| north_providers::ledger::EstimatedRun { tier: "opus-high".into(), category: "debugging-unknown-cause".into(), minutes: *minutes, eta_min: *eta_min, item: None, usd: 1.0, estimated_usd: None, started: Some(ended - (*minutes * 60.0) as i64), ended: north_foundation::foundation::clock::Timestamp::from_unix_seconds(ended).iso() };
    pairs.iter().map(at).collect()
}

fn week_after(week: &[(f64, f64)], earlier: &[(f64, f64)]) -> EstimatedRuns {
    EstimatedRuns { week: estimated(week, 10 * DAY), earlier: estimated(earlier, 5 * DAY) }
}

#[test]
fn an_eta_that_covers_the_quantile_it_targets_is_calibrated_though_the_median_run_sits_far_below_it() {
    let cell = &super::estimates::calibrations(&week_after(&RECORDED_OPUS_HIGH_DEBUGGING, &[]))[0];
    let (low, high) = super::estimates::CALIBRATED;
    assert!(!(low..=high).contains(&cell.minutes.stated.median), "the median ratio {} is the reading that filed the cell as drifted", cell.minutes.stated.median);
    assert!(!cell.drifted(), "the ETA covers its p{} of the recorded runs at {}x, so it holds", cell.minutes.stated.percent, cell.minutes.stated.covered);
}

/// Reproduces north#1306: an ETA re-sized from the cell's own runs of the 7 days before each run
/// still sat outside the band when the week's runs finished faster than the week before's.
#[test]
fn an_eta_drifts_only_where_its_cells_earlier_runs_would_have_covered_its_runs() {
    let short: Vec<(f64, f64)> = RECORDED_OPUS_HIGH_DEBUGGING.iter().map(|(minutes, eta)| (*minutes, eta / 4.0)).collect();
    assert!(super::estimates::calibrations(&week_after(&short, &RECORDED_OPUS_HIGH_DEBUGGING))[0].drifted(), "an ETA a quarter of what its cell's earlier runs would have set is stale");
    let faster: Vec<(f64, f64)> = RECORDED_OPUS_HIGH_DEBUGGING.iter().map(|(minutes, eta)| (minutes / 4.0, *eta)).collect();
    let cells = super::estimates::calibrations(&week_after(&faster, &RECORDED_OPUS_HIGH_DEBUGGING));
    assert!(!cells[0].drifted(), "{}", cells[0].line());
    assert_eq!(super::estimates::noise_lines(&cells)[0], "outside the band under every estimator: 1 cells");
    let priced = |runs: Vec<north_providers::ledger::EstimatedRun>, tier: &str, usd: f64, budget: f64, per_box: usize| -> Vec<north_providers::ledger::EstimatedRun> {
        runs.into_iter().enumerate().map(|(n, run)| north_providers::ledger::EstimatedRun { tier: tier.into(), item: Some(format!("{}#{}", run.ended, n / per_box)), usd, estimated_usd: Some(budget), ..run }).collect()
    };
    let at = |runs: usize, ended: i64| estimated(&vec![(60.0, 60.0); runs], ended);
    let week = [priced(at(3, 10 * DAY), "haiku", 0.1, 1.0, 1), priced(at(9, 10 * DAY), "sonnet-high", 1.0, 3.0, 3), priced(at(3, 10 * DAY), "opus-high", 1.0, 0.2, 1)].concat();
    let earlier = [priced(at(3, 5 * DAY), "haiku", 0.1, 1.0, 1), priced(at(5, 5 * DAY), "opus-high", 1.0, 1.0, 1), priced(at(9, 5 * DAY), "sonnet-high", 1.0, 3.0, 3)].concat();
    let cells = super::estimates::calibrations(&EstimatedRuns { week, earlier });
    for tier in ["haiku", "sonnet-high"] {
        let cell = cells.iter().find(|cell| cell.tier == tier).unwrap_or_else(|| panic!("{cells:#?}"));
        assert!(!cell.drifted(), "a Budget raised to the median box, or stated for a box of several runs, is replayed in its own form: {}", cell.line());
    }
    let stale = cells.iter().find(|cell| cell.tier == "opus-high").unwrap_or_else(|| panic!("{cells:#?}"));
    assert!(stale.drifted(), "a Budget a fifth of what the cell's earlier boxes would have set is stale: {}", stale.line());
}

#[test]
fn an_estimate_drift_is_told_once_when_its_cells_change_and_again_when_it_clears() {
    let drifted = week_after(&[(60.0, 10.0), (70.0, 10.0), (80.0, 10.0)], &[(60.0, 60.0), (70.0, 60.0), (80.0, 60.0)]);
    let ledger = Ledger { week: report(&[], "2026-10-20T06:00:00Z"), before: report(&[], "2026-10-13T06:00:00Z"), estimates: Ok(drifted), briers: Ok(Vec::new()), runs: Ok(Vec::new()), skills: Vec::new() };
    let mut issues = Issues::default();
    let dry = tick(&ledger, &Prior::default(), &[], &mut issues, AT, true);
    let told = format!("north tune: {}: opus-high debugging-unknown-cause; `north tune --dry-run` shows them.", super::estimates::TITLE);
    assert!(issues.told.is_empty() && dry.lines.contains(&format!("  would tell the coordinator: {told}")), "{:#?}", dry.lines);
    let (_, ticked) = two_ticks(&ledger, &mut issues);
    assert_eq!(issues.told, [told], "{:#?}", ticked.lines);
    let calm = Ledger { estimates: Ok(EstimatedRuns::default()), ..ledger };
    let cleared = tick(&calm, &Prior { alarms: ticked.alarms, ..Prior::default() }, &[], &mut issues, AT, false);
    assert_eq!(issues.told.len(), 2, "{:#?}", cleared.lines);
    assert_eq!(issues.told[1], format!("north tune: {}: no cell any longer; `north tune --dry-run` shows the week's cells.", super::estimates::TITLE));
    assert!(cleared.alarms.is_empty() && !cleared.failed, "{:#?}", cleared.lines);
}

const DIVIDENDS: &str = r#"
[[dividend]]
name = "reading above zero"
issue = "north#112"
metric = "usd_per_closed"
at_most = 0.0

[[dividend]]
name = "reading at least zero"
issue = "north#1"
metric = "usd_per_closed"
at_least = 0.0
"#;

fn readings(rows: &[serde_json::Value]) -> Vec<Reading> {
    rows.iter().filter_map(Reading::read).collect()
}

/// Reproduces the guarantee north#112 box 3 observes: one reading per dividend a day, a decay told to
/// the coordinator once, and a decay whose message did not go retried rather than dropped.
#[test]
fn a_dividend_is_read_once_a_day_and_one_decay_tells_the_coordinator_once_until_it_pays_again() {
    let dividends = dividends::parse(DIVIDENDS).unwrap();
    let week = report(&[run("north#1", SONNET, "feature", "2026-10-21T10:00:00Z", "2026-10-21T10:30:00Z")], "2026-10-20T06:00:00Z");
    let mut issues = Issues::default();

    let first = dividends::check(&week, &dividends, &[], AT);
    assert!(first.lines.iter().any(|line| line.starts_with("dividend north#112 reading above zero:") && line.ends_with("DECAYED")), "{:#?}", first.lines);
    assert!(first.lines.iter().any(|line| line.starts_with("dividend north#1 reading at least zero:") && line.ends_with("PAYING")), "{:#?}", first.lines);
    assert_eq!(first.rows.len(), 2, "one reading for each dividend");
    assert_eq!(first.alerts.len(), 1, "only the dividend past its bar is told");
    let (rows, failed) = tell_decays(&mut issues, first.rows, first.alerts, false, &mut Vec::new());
    assert!(!failed && rows.len() == 2 && issues.told.len() == 1, "{:?} {:#?}", issues.told, first.lines);
    assert!(issues.told[0].starts_with("dividend DECAYED: reading above zero (north#112)") && issues.told[0].contains("it was first reading."), "{}", issues.told[0]);

    let mut earlier = readings(&rows);
    let rerun = dividends::check(&week, &dividends, &earlier, AT);
    assert!(rerun.rows.is_empty() && rerun.alerts.is_empty(), "the same day is not read twice: {} rows, {} alerts", rerun.rows.len(), rerun.alerts.len());

    for (day, at) in ["2026-10-28", "2026-10-29", "2026-10-30", "2026-10-31", "2026-11-01", "2026-11-02"].into_iter().enumerate() {
        let checked = dividends::check(&week, &dividends, &earlier, &format!("{at}T06:00:00Z"));
        assert_eq!(checked.rows.len(), 2, "day {day} reads each dividend once");
        assert!(checked.alerts.is_empty(), "still decayed on day {at}: no second message");
        let (next_rows, _) = tell_decays(&mut issues, checked.rows, checked.alerts, false, &mut Vec::new());
        earlier = readings(&next_rows);
    }
    assert_eq!(issues.told.len(), 1, "a week of readings tells the one decay once");

    let mut recovering = dividends.clone();
    recovering[0].bar = Bar::AtLeast(0.0);
    let paid = dividends::check(&week, &recovering, &earlier, "2026-11-03T06:00:00Z");
    assert!(paid.alerts.is_empty() && paid.lines.iter().all(|line| !line.ends_with("DECAYED")), "{:#?}", paid.lines);
    let (paid_rows, _) = tell_decays(&mut issues, paid.rows, paid.alerts, false, &mut Vec::new());

    let again = dividends::check(&week, &dividends, &readings(&paid_rows), "2026-11-04T06:00:00Z");
    assert_eq!(again.alerts.len(), 1, "decaying again after paying tells again");
    assert!(again.alerts[0].1.contains("it was PAYING on 2026-11-03"), "{}", again.alerts[0].1);

    let mut mute = Issues { mute: true, ..Issues::default() };
    let missed = dividends::check(&week, &dividends, &[], AT);
    let (kept, failed) = tell_decays(&mut mute, missed.rows, missed.alerts, false, &mut Vec::new());
    assert!(failed && kept.len() == 1, "a decay the coordinator was not told of keeps no row, so the next tick tells again: {:?}", kept);
    let mut issues = Issues::default();
    let dry = dividends::check(&week, &dividends, &[], AT);
    let (kept, failed) = tell_decays(&mut issues, dry.rows, dry.alerts, true, &mut Vec::new());
    assert!(!failed && kept.len() == 2 && issues.told.is_empty(), "a dry run tells no one and keeps every row");

    let registered = dividends::definitions().unwrap();
    assert!(registered.len() >= 3 && registered.iter().all(|dividend| dividend.issue.starts_with("north#")), "{registered:?}");
    assert!(dividends::parse("[[dividend]]\nname = \"x\"\nissue = \"north#1\"\nmetric = \"usd_per_closed\"\n").is_err());
}
