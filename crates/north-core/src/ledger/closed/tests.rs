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
        reworked: false,
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
fn runs_per_closed_counts_every_worker_run_on_a_closed_landed_box_by_category() {
    let prices = PriceTable::load().unwrap();
    let report = summarize(&fixture(), &closed(&["north#1", "north#2", "north#3", "north#5"]), "2026-10-27T00:00:00Z", &prices);
    assert_eq!(report.runs_per_closed(), [("feature".to_owned(), 2, 1), ("tooling".to_owned(), 2, 2)]);
    assert_eq!(report.runs_line("24h"), "runs/closed 24h: feature 2.00 (2/1) | tooling 1.00 (2/2)");
    assert_eq!(report.json("24h")["runs_per_closed"][0], json!({"category": "feature", "runs": 2, "closed": 1, "runs_per_closed": 2.0}));
    let none = summarize(&fixture(), &closed(&[]), "2026-10-27T00:00:00Z", &prices);
    assert_eq!(none.runs_line("24h"), "runs/closed 24h: -");
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
fn an_arm_box_costs_its_whole_spend_to_the_tier_and_arm_of_its_first_run_whether_or_not_it_closed() {
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
    let (arms, open): (Vec<&ArmBox>, Vec<&ArmBox>) = report.arms.iter().partition(|armed| armed.closed);
    assert_eq!(open.iter().map(|armed| (armed.item.as_str(), armed.arm.as_str(), armed.category.as_str())).collect::<Vec<_>>(), [("north#4", "no-handoff", "tooling")], "an unclosed armed box is listed so its arm's closed-box rate and spend count it");
    assert!((open[0].usd - SONNET).abs() < 1e-9, "{open:?}");
    assert!(arms.iter().all(|armed| armed.category == "tooling" && !armed.reworked), "{arms:?}");
    let arms: Vec<(&str, &str, &str, f64)> = arms.iter().map(|closed| (closed.item.as_str(), closed.tier.as_str(), closed.arm.as_str(), closed.usd)).collect();
    assert_eq!(arms.len(), 3, "{arms:?}");
    assert_eq!((arms[0].0, arms[0].1, arms[0].2), ("north#1", "sonnet", "handoff-350k"));
    assert!((arms[0].3 - 2.0 * SONNET).abs() < 1e-9, "{arms:?}");
    assert_eq!(arms[1..].iter().map(|closed| (closed.0, closed.2)).collect::<Vec<_>>(), [("north#2", "no-handoff"), ("north#3", "no-handoff")]);
    assert!(arms[1..].iter().all(|closed| (closed.3 - SONNET).abs() < 1e-9), "{arms:?}");
    let printed: Vec<ArmBox> = serde_json::from_value(report.json("7d")["arms"].clone()).unwrap();
    assert_eq!(printed, report.arms, "ledger summary --json prints each arm box for another host's dispatch to price");
}

#[test]
fn the_json_lists_each_closed_box_of_a_cell_with_its_spend() {
    let prices = PriceTable::load().unwrap();
    let report = summarize(&fixture(), &closed(&["north#2", "north#3"]), "2026-10-27T00:00:00Z", &prices);
    let cell = &report.json("7d")["cells"][0];
    let items: Vec<&str> = cell["boxes"].as_array().unwrap().iter().map(|entry| entry["item"].as_str().unwrap()).collect();
    assert_eq!(items, ["north#2", "north#3"]);
    assert!(cell["boxes"].as_array().unwrap().iter().all(|entry| (entry["usd"].as_f64().unwrap() - SONNET).abs() < 1e-9));
}

#[test]
fn spend_on_boxes_started_in_the_window_that_never_closed_joins_the_cost_per_closed_box_of_their_cell() {
    let prices = PriceTable::load().unwrap();
    let (sonnet, opus) = ("claude-sonnet-5-5", "claude-opus-5-5");
    let runs = vec![
        run(Some("north#2"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T11:00:00Z", true),
        run(Some("north#8"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T13:00:00Z", false),
        run(Some("north#8"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T14:00:00Z", false),
        run(Some("north#9"), "worker", "medium", "feature", opus, "2026-10-27T15:00:00Z", false),
        run(Some("north#7"), "worker", "sonnet", "tooling", sonnet, "2026-10-20T12:00:00Z", false),
        run(Some("north#6"), "worker", "sonnet", "tooling", sonnet, "2026-10-27T16:00:00Z", false),
        run(Some("north#6"), "reviewer", "sonnet", "unknown", sonnet, "2026-10-27T16:30:00Z", false),
    ];
    let report = summarize(&runs, &closed(&["north#2"]), "2026-10-27T00:00:00Z", &prices);
    let cell = &report.cells[0];
    assert_eq!((cell.tier.as_str(), cell.category.as_str(), cell.closed), ("sonnet", "tooling", 1));
    let approx = |left: f64, right: f64| (left - right).abs() < 1e-9;
    assert!(approx(cell.usd, SONNET), "{}", cell.usd);
    assert!(approx(cell.unclosed_usd, 3.0 * SONNET), "{}", cell.unclosed_usd);
    assert!(approx(cell.usd_per_closed(), 4.0 * SONNET), "{}", cell.usd_per_closed());
    assert_eq!(report.cells.len(), 1);
}
