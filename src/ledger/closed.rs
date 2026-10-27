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
    pub model: Option<String>,
    pub started: String,
    pub ended: String,
    pub landed: bool,
    pub input: i64,
    pub cache_read: i64,
    pub output: i64,
}

#[derive(Debug, PartialEq)]
pub struct Cell {
    pub tier: String,
    pub category: String,
    pub closed: usize,
    pub usd: f64,
    box_usd: Vec<f64>,
    run_minutes: Vec<f64>,
}

impl Cell {
    #[cfg(test)]
    pub fn priced(tier: &str, category: &str, closed: usize, usd: f64) -> Self {
        Self { tier: tier.into(), category: category.into(), closed, usd, box_usd: Vec::new(), run_minutes: Vec::new() }
    }

    pub fn usd_per_closed(&self) -> f64 {
        self.usd / self.closed as f64
    }
}

/// What a closed-box cell says about its next box: the median cost of a closed box and the p75 wall
/// of a run on one, each with the sample it comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct Figures {
    pub closed: usize,
    pub median_usd: f64,
    pub runs: usize,
    pub p75_minutes: f64,
}

fn percentile(values: &mut [f64], percent: usize) -> f64 {
    values.sort_by(f64::total_cmp);
    values[(values.len() * percent).div_ceil(100).max(1) - 1]
}

#[derive(Debug, PartialEq)]
pub struct ClosedReport {
    pub cells: Vec<Cell>,
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
    let mut cells: HashMap<(String, String), Cell> = HashMap::new();
    for runs in boxes.values_mut().filter(|runs| runs.iter().any(|run| run.landed)) {
        runs.sort_by(|a, b| (a.started.as_str(), a.ended.as_str()).cmp(&(b.started.as_str(), b.ended.as_str())));
        let category = runs.iter().map(|run| run.category.as_str()).find(|category| *category != "unknown").unwrap_or("unknown");
        let key = (tier_label(&runs[0].tier), category.to_owned());
        let cell = cells.entry(key.clone()).or_insert(Cell { tier: key.0, category: key.1, closed: 0, usd: 0.0, box_usd: Vec::new(), run_minutes: Vec::new() });
        let spent = runs.iter().map(|run| run.usd(prices)).sum::<f64>();
        cell.closed += 1;
        cell.usd += spent;
        cell.box_usd.push(spent);
        cell.run_minutes.extend(runs.iter().filter_map(|run| run.minutes()));
    }
    let mut cells: Vec<Cell> = cells.into_values().collect();
    cells.sort_by(|a, b| (&a.tier, &a.category).cmp(&(&b.tier, &b.category)));
    let window = || runs.iter().filter(|run| run.ended.as_str() >= since);
    ClosedReport {
        closed: cells.iter().map(|cell| cell.closed).sum(),
        cells,
        spend_usd: window().map(|run| run.usd(prices)).sum(),
        leads_usd: window().filter(|run| LEADS.contains(&run.role.as_str())).map(|run| run.usd(prices)).sum(),
    }
}

impl ClosedReport {
    fn per_closed(&self, usd: f64) -> Option<f64> {
        (self.closed > 0).then(|| usd / self.closed as f64)
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
            runs: walls.len(),
            p75_minutes: percentile(&mut walls, 75),
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
