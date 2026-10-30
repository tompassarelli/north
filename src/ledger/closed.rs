use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

use super::pricing::{Price, PriceTable, TokenUse};

const LEADS: [&str; 2] = ["parent", "proxy"];
const WORKER: &str = "worker";

#[derive(Clone, Debug, PartialEq)]
pub struct SpendRun {
    pub item: Option<String>,
    pub role: String,
    pub tier: String,
    pub category: String,
    pub arm: String,
    pub model: Option<String>,
    pub started: String,
    pub ended: String,
    pub landed: bool,
    pub input: i64,
    pub cache_read: i64,
    pub output: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArmCell {
    pub tier: String,
    pub arm: String,
    pub closed: usize,
    pub usd: f64,
}

#[derive(Debug, PartialEq)]
pub struct Cell {
    pub tier: String,
    pub category: String,
    pub closed: usize,
    pub usd: f64,
    pub staffed: bool,
    box_usd: Vec<f64>,
    run_minutes: Vec<f64>,
}

impl Cell {
    #[cfg(test)]
    pub fn priced(tier: &str, category: &str, closed: usize, usd: f64) -> Self {
        Self { tier: tier.into(), category: category.into(), closed, usd, staffed: true, box_usd: Vec::new(), run_minutes: Vec::new() }
    }

    pub fn usd_per_closed(&self) -> f64 {
        self.usd / self.closed as f64
    }
}

/// What a closed-box cell says about its next box: the median cost of a closed box and the p75 wall
/// of a run on one, each with the sample it comes from and its 95% order-statistic interval.
#[derive(Clone, Debug, PartialEq)]
pub struct Figures {
    pub closed: usize,
    pub median_usd: f64,
    pub usd_interval: (f64, f64),
    pub runs: usize,
    pub p75_minutes: f64,
    pub minutes_interval: (f64, f64),
}

fn percentile(values: &mut [f64], percent: usize) -> f64 {
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

#[derive(Debug, PartialEq)]
pub struct ClosedReport {
    pub cells: Vec<Cell>,
    pub arms: Vec<ArmCell>,
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
        let price = self.model.as_deref().and_then(|model| prices.price(model)).or_else(|| prices.price(fallback_model(&self.tier)));
        price.map_or(0.0, |price: Price| {
            price.of(TokenUse { input: 0, output: self.output, cache_read: self.cache_read, cache_write: (self.input - self.cache_read).max(0) })
        })
    }
}

fn fallback_model(tier: &str) -> &'static str {
    if tier.contains("haiku") {
        "haiku"
    } else if tier.contains("sonnet") {
        "sonnet"
    } else {
        "opus"
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
    let mut arms: HashMap<(String, String), ArmCell> = HashMap::new();
    for runs in boxes.values_mut().filter(|runs| runs.iter().any(|run| run.landed)) {
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let key = (tier_label(&runs[0].tier), category.to_owned());
        let cell = cells.entry(key.clone()).or_insert(Cell { staffed: staffed.contains(&key.0), tier: key.0, category: key.1, closed: 0, usd: 0.0, box_usd: Vec::new(), run_minutes: Vec::new() });
        let spent = runs.iter().map(|run| run.usd(prices)).sum::<f64>();
        cell.closed += 1;
        cell.usd += spent;
        cell.box_usd.push(spent);
        cell.run_minutes.extend(runs.iter().filter_map(|run| run.minutes()));
        if !runs[0].arm.is_empty() {
            let key = (tier_label(&runs[0].tier), runs[0].arm.clone());
            let arm = arms.entry(key.clone()).or_insert(ArmCell { tier: key.0, arm: key.1, closed: 0, usd: 0.0 });
            arm.closed += 1;
            arm.usd += spent;
        }
    }
    let mut cells: Vec<Cell> = cells.into_values().collect();
    cells.sort_by(|a, b| (&a.tier, &a.category).cmp(&(&b.tier, &b.category)));
    let mut arms: Vec<ArmCell> = arms.into_values().collect();
    arms.sort_by(|a, b| (&a.tier, &a.arm).cmp(&(&b.tier, &b.arm)));
    let window = || runs.iter().filter(|run| run.ended.as_str() >= since);
    ClosedReport {
        closed: cells.iter().map(|cell| cell.closed).sum(),
        cells,
        arms,
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
        let mut spends: Vec<f64> = self.cells.iter().flat_map(|cell| cell.box_usd.iter().copied()).collect();
        (!spends.is_empty()).then(|| (percentile(&mut spends, 50), spends.len()))
    }

    /// The figures of every cell in `category` whose tier label names `family` (`haiku`, `sonnet` or
    /// `opus`), pooled; None when no box closed there or no run of it has a wall time.
    pub fn figures(&self, family: &str, category: &str) -> Option<Figures> {
        let cells: Vec<&Cell> = self.cells.iter().filter(|cell| cell.category == category && cell.tier.contains(family)).collect();
        let mut spends: Vec<f64> = cells.iter().flat_map(|cell| cell.box_usd.iter().copied()).collect();
        let mut walls: Vec<f64> = cells.iter().flat_map(|cell| cell.run_minutes.iter().copied()).collect();
        (!spends.is_empty() && !walls.is_empty()).then(|| Figures {
            closed: spends.len(),
            median_usd: percentile(&mut spends, 50),
            usd_interval: interval(&mut spends, 50),
            runs: walls.len(),
            p75_minutes: percentile(&mut walls, 75),
            minutes_interval: interval(&mut walls, 75),
        })
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
                "tier": cell.tier, "category": cell.category, "closed": cell.closed, "usd": cell.usd, "usd_per_closed": cell.usd_per_closed(),
            })).collect::<Vec<_>>(),
        })
    }

    pub fn print_table(&self) {
        println!("tier\tcategory\tclosed\tusd\tusd_per_closed");
        for cell in &self.cells {
            println!("{}\t{}\t{}\t{:.2}\t{:.2}", cell.tier, cell.category, cell.closed, cell.usd, cell.usd_per_closed());
        }
    }
}

#[cfg(test)]
mod tests;
