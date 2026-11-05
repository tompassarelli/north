use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

use crate::router::pricing::PriceTable;

const LEADS: [&str; 2] = ["parent", "proxy"];
const WORKER: &str = "worker";

#[derive(Clone, Debug, PartialEq)]
pub struct SpendRun {
    pub agent: String,
    pub item: Option<String>,
    pub role: String,
    pub tier: String,
    pub category: String,
    pub arm: String,
    pub model: Option<String>,
    pub started: String,
    pub ended: String,
    pub landed: bool,
    pub reworked: bool,
    pub input: i64,
    pub cache_read: i64,
    pub output: i64,
    pub skills: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ArmBox {
    pub item: String,
    pub tier: String,
    pub arm: String,
    pub usd: f64,
    #[serde(default)]
    pub category: String,
    #[serde(default = "closed_by_default")]
    pub closed: bool,
    #[serde(default)]
    pub reworked: bool,
}

fn closed_by_default() -> bool {
    true
}

/// A closed box with what it cost and the worker runs that cost it, in the order they started.
#[derive(Clone, Debug, PartialEq)]
pub struct ClosedBox {
    pub item: String,
    pub usd: f64,
    pub runs: Vec<BoxRun>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxRun {
    pub agent: String,
    pub tier: String,
    pub category: String,
    pub started: String,
    pub usd: f64,
}

/// One box the window's worker runs spent on, closed or not, with the skills any of its runs loaded.
#[derive(Clone, Debug, PartialEq)]
pub struct SkillBox {
    pub item: String,
    pub category: String,
    pub usd: f64,
    pub closed: bool,
    pub skills: BTreeSet<String>,
}

impl SkillBox {
    fn of(item: &str, category: &str, usd: f64, closed: bool, runs: &[&SpendRun]) -> Self {
        Self { item: item.to_owned(), category: category.to_owned(), usd, closed, skills: runs.iter().flat_map(|run| run.skills.iter().cloned()).collect() }
    }
}

#[derive(Debug, PartialEq)]
pub struct Cell {
    pub tier: String,
    pub category: String,
    pub closed: usize,
    pub usd: f64,
    pub staffed: bool,
    pub unclosed_usd: f64,
    pub reworked: usize,
    pub runs: usize,
    boxes: Vec<ClosedBox>,
    run_minutes: Vec<f64>,
}

impl Cell {
    #[doc(hidden)]
    pub fn priced(tier: &str, category: &str, closed: usize, usd: f64) -> Self {
        Self { tier: tier.into(), category: category.into(), closed, usd, staffed: true, unclosed_usd: 0.0, reworked: 0, runs: 0, boxes: Vec::new(), run_minutes: Vec::new() }
    }

    pub fn usd_per_closed(&self) -> f64 {
        (self.usd + self.unclosed_usd) / self.closed as f64
    }

    pub fn kept(&self) -> usize {
        self.closed - self.reworked
    }

    /// The median cost of one closed box of the cell, which one umbrella issue with hundreds of runs does not move; None when no box is recorded.
    pub fn median_box_usd(&self) -> Option<f64> {
        let mut spends: Vec<f64> = self.boxes.iter().map(|closed| closed.usd).collect();
        (!spends.is_empty()).then(|| percentile(&mut spends, 50))
    }

    /// The closed boxes of the cell with what each cost, dearest first.
    pub fn boxes_dearest_first(&self) -> Vec<ClosedBox> {
        let mut boxes = self.boxes.clone();
        boxes.sort_by(|a, b| b.usd.total_cmp(&a.usd));
        boxes
    }
}

/// What a closed-box cell says about its next box: the median and p90 cost of a closed box and the p75 wall
/// of a run on one, each with the sample it comes from and its 95% order-statistic interval.
#[derive(Clone, Debug, PartialEq)]
pub struct Figures {
    pub closed: usize,
    pub median_usd: f64,
    pub p90_usd: f64,
    pub usd_interval: (f64, f64),
    pub runs: usize,
    pub p75_minutes: f64,
    pub minutes_interval: (f64, f64),
}

/// The nearest-rank `percent` percentile of `values`, which it sorts; `values` must not be empty.
pub fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * percent).div_ceil(100).max(1) - 1]
}

/// The distribution-free 95% interval of a quantile: the sorted sample's values at the 1-based
/// ranks ⌊n·q − 1.96·√(n·q·(1−q))⌋ and ⌈n·q + 1.96·√(n·q·(1−q))⌉, clamped to the sample.
fn interval(values: &mut [f64], percent: usize) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let (n, q) = (values.len() as f64, percent as f64 / 100.0);
    let spread = 1.96 * (n * q * (1.0 - q)).sqrt();
    let rank = |at: f64| (at.clamp(1.0, n) as usize) - 1;
    (values[rank((n * q - spread).floor())], values[rank((n * q + spread).ceil())])
}

#[derive(Debug, Default, PartialEq)]
pub struct ClosedReport {
    pub cells: Vec<Cell>,
    /// Repositories outside the catalog whose landed runs could not be matched to a closed issue, with the count of those runs.
    pub skipped: Vec<(String, usize)>,
    pub arms: Vec<ArmBox>,
    pub boxes: Vec<SkillBox>,
    /// The runs of every role that ended in the window and loaded each skill, by skill.
    pub loaded: BTreeMap<String, usize>,
    pub closed: usize,
    pub spend_usd: f64,
    pub leads_usd: f64,
}

impl SpendRun {
    fn minutes(&self) -> Option<f64> {
        let wall = Timestamp::parse(&self.ended).ok()?.unix_seconds() - Timestamp::parse(&self.started).ok()?.unix_seconds();
        (wall >= 0).then(|| wall as f64 / 60.0)
    }

    fn usd(&self, prices: &PriceTable) -> f64 {
        prices.run_usd(self.model.as_deref(), &self.tier, self.input, self.cache_read, self.output)
    }
}

pub fn tier_label(tier: &str) -> String {
    match tier {
        "low" | "medium" | "high" | "xhigh" | "max" => format!("opus-{tier}"),
        other => other.to_owned(),
    }
}

pub fn summarize(runs: &[SpendRun], closed: &HashSet<String>, since: &str, prices: &PriceTable) -> ClosedReport {
    let mut boxes: BTreeMap<&str, Vec<&SpendRun>> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.role == WORKER) {
        if let Some(item) = run.item.as_deref().filter(|item| closed.contains(*item)) {
            boxes.entry(item).or_default().push(run);
        }
    }
    let staffed: HashSet<String> = runs.iter().filter(|run| run.role == WORKER && run.started.as_str() >= since).map(|run| tier_label(&run.tier)).collect();
    let mut cells: HashMap<(String, String), Cell> = HashMap::new();
    let mut arms = Vec::new();
    let mut spent_boxes = Vec::new();
    for (item, runs) in boxes.iter_mut().filter(|(_, runs)| runs.iter().any(|run| run.landed)) {
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let key = (tier_label(&runs[0].tier), category.to_owned());
        let cell = cells.entry(key.clone()).or_insert(Cell { staffed: staffed.contains(&key.0), tier: key.0, category: key.1, closed: 0, usd: 0.0, unclosed_usd: 0.0, reworked: 0, runs: 0, boxes: Vec::new(), run_minutes: Vec::new() });
        let priced: Vec<BoxRun> = runs.iter().map(|run| BoxRun { agent: run.agent.clone(), tier: tier_label(&run.tier), category: run.category.clone(), started: run.started.clone(), usd: run.usd(prices) }).collect();
        let spent = priced.iter().map(|run| run.usd).sum::<f64>();
        cell.closed += 1;
        cell.runs += runs.len();
        let reworked = runs.iter().any(|run| run.landed && run.reworked);
        cell.reworked += usize::from(reworked);
        cell.usd += spent;
        cell.boxes.push(ClosedBox { item: (*item).to_owned(), usd: spent, runs: priced });
        spent_boxes.push(SkillBox::of(item, category, spent, true, runs));
        cell.run_minutes.extend(runs.iter().filter_map(|run| run.minutes()));
        if !runs[0].arm.is_empty() {
            arms.push(ArmBox { item: (*item).to_owned(), tier: tier_label(&runs[0].tier), arm: runs[0].arm.clone(), usd: spent, category: category.to_owned(), closed: true, reworked });
        }
    }
    let mut open: BTreeMap<&str, Vec<&SpendRun>> = BTreeMap::new();
    for run in runs.iter().filter(|run| run.role == WORKER) {
        if let Some(item) = run.item.as_deref().filter(|item| !boxes.get(*item).is_some_and(|runs| runs.iter().any(|run| run.landed))) {
            open.entry(item).or_default().push(run);
        }
    }
    for (item, runs) in open.iter_mut() {
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        if runs[0].started.as_str() < since {
            continue;
        }
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let spent = runs.iter().map(|run| run.usd(prices)).sum::<f64>();
        spent_boxes.push(SkillBox::of(item, category, spent, false, runs));
        if let Some(cell) = cells.get_mut(&(tier_label(&runs[0].tier), category.to_owned())) {
            cell.unclosed_usd += spent;
        }
        if !runs[0].arm.is_empty() {
            arms.push(ArmBox { item: (*item).to_owned(), tier: tier_label(&runs[0].tier), arm: runs[0].arm.clone(), usd: spent, category: category.to_owned(), closed: false, reworked: false });
        }
    }
    let mut cells: Vec<Cell> = cells.into_values().collect();
    cells.sort_by(|a, b| (&a.tier, &a.category).cmp(&(&b.tier, &b.category)));
    let window = || runs.iter().filter(|run| run.ended.as_str() >= since);
    let mut loaded = BTreeMap::new();
    for skill in window().flat_map(|run| &run.skills) {
        *loaded.entry(skill.clone()).or_default() += 1;
    }
    ClosedReport {
        skipped: Vec::new(),
        closed: cells.iter().map(|cell| cell.closed).sum(),
        cells,
        arms,
        boxes: spent_boxes,
        loaded,
        spend_usd: window().map(|run| run.usd(prices)).sum(),
        leads_usd: window().filter(|run| LEADS.contains(&run.role.as_str())).map(|run| run.usd(prices)).sum(),
    }
}

impl ClosedReport {
    fn per_closed(&self, usd: f64) -> Option<f64> {
        (self.closed > 0).then(|| usd / self.closed as f64)
    }

    pub fn all_usd_per_closed(&self) -> Option<f64> {
        self.per_closed(self.spend_usd)
    }

    pub fn leads_usd_per_closed(&self) -> Option<f64> {
        self.per_closed(self.leads_usd)
    }

    /// The share of the closed boxes' spend in `categories` that cells whose tier label names
    /// `family` hold; None when those categories closed no box.
    pub fn family_share(&self, family: &str, categories: &[String]) -> Option<f64> {
        let cells = || self.cells.iter().filter(|cell| categories.contains(&cell.category));
        let total: f64 = cells().map(|cell| cell.usd).sum();
        (total > 0.0).then(|| cells().filter(|cell| cell.tier.contains(family)).map(|cell| cell.usd).sum::<f64>() / total)
    }

    /// The median cost of one closed box across every cell, and how many boxes closed; None when
    /// none did.
    pub fn median_box_usd(&self) -> Option<(f64, usize)> {
        let mut spends: Vec<f64> = self.cells.iter().flat_map(|cell| cell.boxes.iter().map(|closed| closed.usd)).collect();
        (!spends.is_empty()).then(|| (percentile(&mut spends, 50), spends.len()))
    }

    /// The figures of every cell in `category` whose tier label names `family` (`haiku`, `sonnet` or
    /// `opus`), pooled; None when no box closed there or no run of it has a wall time.
    pub fn figures(&self, family: &str, category: &str) -> Option<Figures> {
        let cells: Vec<&Cell> = self.cells.iter().filter(|cell| cell.category == category && cell.tier.contains(family)).collect();
        let mut spends: Vec<f64> = cells.iter().flat_map(|cell| cell.boxes.iter().map(|closed| closed.usd)).collect();
        let mut walls: Vec<f64> = cells.iter().flat_map(|cell| cell.run_minutes.iter().copied()).collect();
        (!spends.is_empty() && !walls.is_empty()).then(|| Figures {
            closed: spends.len(),
            median_usd: percentile(&mut spends, 50),
            p90_usd: percentile(&mut spends, 90),
            usd_interval: interval(&mut spends, 50),
            runs: walls.len(),
            p75_minutes: percentile(&mut walls, 75),
            minutes_interval: interval(&mut walls, 75),
        })
    }

    /// Worker runs per closed box in each category, pooled over its tiers: (category, runs, closed boxes).
    pub fn runs_per_closed(&self) -> Vec<(String, usize, usize)> {
        let mut categories: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for cell in &self.cells {
            let tally = categories.entry(&cell.category).or_default();
            tally.0 += cell.runs;
            tally.1 += cell.closed;
        }
        categories.into_iter().map(|(category, (runs, closed))| (category.to_owned(), runs, closed)).collect()
    }

    pub fn runs_line(&self, window: &str) -> String {
        let parts: Vec<String> = self.runs_per_closed().iter().map(|(category, runs, closed)| format!("{category} {:.2} ({runs}/{closed})", *runs as f64 / *closed as f64)).collect();
        format!("runs/closed {window}: {}", if parts.is_empty() { "-".to_owned() } else { parts.join(" | ") })
    }

    pub fn dashboard(&self, window: &str) -> String {
        let money = |usd: Option<f64>| usd.map_or_else(|| "-".to_owned(), |usd| format!("${usd:.2}"));
        let mut parts = vec![format!("all {} ({})", money(self.per_closed(self.spend_usd)), self.closed)];
        parts.extend(self.cells.iter().map(|cell| format!("{} {} {} ({})", cell.tier, cell.category, money(Some(cell.usd_per_closed())), cell.closed)));
        parts.push(format!("leads {}/closed", money(self.per_closed(self.leads_usd))));
        format!("cost/closed {window}: {}", parts.join(" | "))
    }

    pub fn json(&self, window: &str) -> Value {
        json!({
            "since": window,
            "closed": self.closed,
            "all_usd_per_closed": self.per_closed(self.spend_usd),
            "leads_usd_per_closed": self.per_closed(self.leads_usd),
            "cells": self.cells.iter().map(|cell| json!({
                "tier": cell.tier, "category": cell.category, "closed": cell.closed, "usd": cell.usd, "unclosed_usd": cell.unclosed_usd, "reworked": cell.reworked, "usd_per_closed": cell.usd_per_closed(),
                "boxes": cell.boxes.iter().map(|closed| json!({"item": closed.item, "usd": closed.usd, "runs": closed.runs.iter().map(|run| json!({"agent": run.agent, "tier": run.tier, "category": run.category, "started": run.started, "usd": run.usd})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "runs_per_closed": self.runs_per_closed().iter().map(|(category, runs, closed)| json!({"category": category, "runs": runs, "closed": closed, "runs_per_closed": *runs as f64 / *closed as f64})).collect::<Vec<_>>(),
            "arms": self.arms,
            "skipped_repositories": self.skipped.iter().map(|(repo, runs)| json!({"repo": repo, "landed_runs": runs})).collect::<Vec<_>>(),
        })
    }

    /// One line naming the repositories whose landed runs the report left out, or None when it left out none.
    pub fn skipped_line(&self) -> Option<String> {
        let named: Vec<String> = self.skipped.iter().map(|(repo, runs)| format!("{repo} ({runs} landed runs)")).collect();
        (!named.is_empty()).then(|| format!("skipped repositories the catalog does not know, so their boxes are not counted: {}", named.join(", ")))
    }

    pub fn print_table(&self) {
        println!("tier\tcategory\tclosed\tusd\tunclosed_usd\tusd_per_closed");
        for cell in &self.cells {
            println!("{}\t{}\t{}\t{:.2}\t{:.2}\t{:.2}", cell.tier, cell.category, cell.closed, cell.usd, cell.unclosed_usd, cell.usd_per_closed());
        }
    }
}

