use std::collections::HashSet;

use super::*;

fn run(item: Option<&str>, role: &str, tier: &str, category: &str, model: &str, ended: &str, landed: bool) -> SpendRun {
    SpendRun {
        item: item.map(str::to_owned),
        role: role.into(),
        tier: tier.into(),
        category: category.into(),
        arm: String::new(),
        model: Some(model.into()),
        started: ended.into(),
        ended: ended.into(),
        landed,
        input: 3_000_000,
        cache_read: 2_000_000,
        output: 100_000,
    }
}

const OPUS: f64 = 10.4;
const SONNET: f64 = 5.2;
const HAIKU: f64 = 0.27;

fn fixture() -> Vec<SpendRun> {
    let (opus, sonnet, haiku) = ("claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5");
    vec![
        run(Some("north#1"), "worker", "medium", "feature", opus, "2026-10-26T20:00:00Z", false),
        run(Some("north#1"), "worker", "medium", "feature", opus, "2026-10-27T10:00:00Z", true),
        run(Some("north#2"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T11:00:00Z", true),
        run(Some("north#3"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T12:00:00Z", true),
        run(Some("north#4"), "worker", "haiku", "mechanical", haiku, "2026-10-27T12:00:00Z", true),
        run(Some("north#5"), "worker", "haiku", "mechanical", haiku, "2026-10-27T12:00:00Z", false),
        run(None, "proxy", "claude-opus-5-5", "unknown", opus, "2026-10-27T09:00:00Z", false),
    ]
}

fn closed(refs: &[&str]) -> HashSet<String> {
    refs.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn usd_per_closed_sums_every_run_on_a_closed_landed_item_by_the_tier_and_category_of_its_first_run() {
    let prices = PriceTable::load().unwrap();
    let report = summarize(&fixture(), &closed(&["north#1", "north#2", "north#3", "north#5"]), "2026-10-27T00:00:00Z", &prices);
    let figures: Vec<(&str, &str, usize, f64)> = report.cells.iter().map(|cell| (cell.tier.as_str(), cell.category.as_str(), cell.closed, cell.usd_per_closed())).collect();
    let approx = |left: f64, right: f64| (left - right).abs() < 1e-9;
    assert_eq!(figures.len(), 2, "{figures:?}");
    assert_eq!((figures[0].0, figures[0].1, figures[0].2), ("opus-medium", "feature", 1));
    assert!(approx(figures[0].3, 2.0 * OPUS), "{figures:?}");
    assert_eq!((figures[1].0, figures[1].1, figures[1].2), ("sonnet", "tooling", 2));
    assert!(approx(figures[1].3, SONNET), "{figures:?}");
    assert_eq!(report.closed, 3);
    let window_spend = OPUS + SONNET * 2.0 + HAIKU * 2.0 + OPUS;
    assert!(approx(report.spend_usd, window_spend), "{}", report.spend_usd);
    assert!(approx(report.leads_usd, OPUS));
}

#[test]
fn the_dashboard_line_names_all_each_cell_and_the_leads_term() {
    let prices = PriceTable::load().unwrap();
    let report = summarize(&fixture(), &closed(&["north#1", "north#2", "north#3"]), "2026-10-27T00:00:00Z", &prices);
    assert_eq!(
        report.dashboard("24h"),
        "cost/closed 24h: all $10.58 (3) | opus-medium feature $20.80 (1) | sonnet tooling $5.20 (2) | leads $3.47/closed"
    );
}

#[test]
fn no_closed_box_leaves_every_per_closed_figure_empty() {
    let report = summarize(&fixture(), &closed(&[]), "2026-10-27T00:00:00Z", &PriceTable::load().unwrap());
    assert_eq!(report.dashboard("24h"), "cost/closed 24h: all - (0) | leads -/closed");
}

#[test]
fn a_quantile_interval_spans_the_order_statistics_around_its_rank_and_stays_inside_the_sample() {
    let sample = |n: usize| (1..=n).map(|value| value as f64).collect::<Vec<f64>>();
    assert_eq!(interval(&mut sample(1), 50), (1.0, 1.0));
    assert_eq!(interval(&mut sample(2), 50), (1.0, 2.0));
    assert_eq!(interval(&mut sample(20), 50), (5.0, 15.0));
    assert_eq!(interval(&mut sample(100), 50), (40.0, 60.0));
    assert_eq!(interval(&mut sample(100), 75), (66.0, 84.0));
    assert_eq!(interval(&mut [9.0, 1.0, 5.0], 50), (1.0, 9.0), "it sorts first");
}

#[test]
fn the_window_and_the_one_before_it_meet_at_one_instant_from_one_clock_reading() {
    let now = crate::foundation::clock::Timestamp::parse("2026-10-27T06:00:00Z").unwrap();
    let [(since, until), (earlier, start)] = crate::ledger::closed_source::adjacent_windows(now, 7 * 86_400);
    assert_eq!((until.as_str(), start.as_str()), (now.iso().as_str(), since.as_str()));
    assert!(since.starts_with("2026-10-20T06:00:00") && earlier.starts_with("2026-10-13T06:00:00"), "{since} {earlier}");
}

#[test]
fn a_closed_box_costs_its_whole_spend_to_the_tier_and_arm_of_its_first_run() {
    let armed = |item: &str, arm: &str, ended: &str, landed: bool| SpendRun { arm: arm.into(), ..run(Some(item), "worker", "sonnet", "tooling", "claude-sonnet-5-5", ended, landed) };
    let runs = vec![
        armed("north#1", "handoff-350k", "2026-10-27T10:00:00Z", false),
        armed("north#1", "", "2026-10-27T11:00:00Z", true),
        armed("north#2", "no-handoff", "2026-10-27T12:00:00Z", true),
        armed("north#3", "no-handoff", "2026-10-27T13:00:00Z", true),
        armed("north#4", "no-handoff", "2026-10-27T14:00:00Z", true),
        armed("north#5", "", "2026-10-27T15:00:00Z", true),
    ];
    let report = summarize(&runs, &closed(&["north#1", "north#2", "north#3", "north#5"]), "2026-10-27T00:00:00Z", &PriceTable::load().unwrap());
    let arms: Vec<(&str, &str, usize, f64)> = report.arms.iter().map(|cell| (cell.tier.as_str(), cell.arm.as_str(), cell.closed, cell.usd / cell.closed as f64)).collect();
    assert_eq!(arms.len(), 2, "{arms:?}");
    assert_eq!((arms[0].0, arms[0].1, arms[0].2), ("sonnet", "handoff-350k", 1));
    assert!((arms[0].3 - 2.0 * SONNET).abs() < 1e-9, "{arms:?}");
    assert_eq!((arms[1].0, arms[1].1, arms[1].2), ("sonnet", "no-handoff", 2));
    assert!((arms[1].3 - SONNET).abs() < 1e-9, "{arms:?}");
}
