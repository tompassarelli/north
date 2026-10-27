use std::collections::HashSet;

use super::*;

fn run(item: Option<&str>, role: &str, tier: &str, category: &str, model: &str, ended: &str, landed: bool) -> SpendRun {
    SpendRun {
        item: item.map(str::to_owned),
        role: role.into(),
        tier: tier.into(),
        category: category.into(),
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
