//! The dispatch and spend figures of the window: per task category and model mix, its runs, first tries, the cost
//! of a first-try landing and of a closed box, the boxes reworked within 48 h, and the spend of every role by model
//! and effort.
use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use north_dispatch::dispatch::{cells, seed_cell};

use super::Screen;

#[derive(Clone, Debug, Serialize)]
pub struct Spend {
    pub mix: String,
    pub runs: usize,
    pub usd: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Row {
    pub category: String,
    pub mix: String,
    pub tier: String,
    pub runs: usize,
    pub usd: f64,
    pub first_tries: usize,
    pub first_tries_landed: usize,
    pub usd_per_first_try_landed: Option<f64>,
    pub closed: usize,
    pub reworked: usize,
    pub usd_per_closed: Option<f64>,
    /// The mix whose closed box costs most in a category where more than one mix closed a box.
    pub most_per_box: bool,
    #[serde(skip)]
    mixes: BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellRow {
    pub category: String,
    pub cell: String,
    pub role: String,
    pub runs: usize,
    pub first_tries: usize,
    pub first_tries_landed: usize,
    pub share: f64,
}

pub struct Mixes {
    pub spend: Vec<Spend>,
    pub models: Vec<Spend>,
    pub rows: Vec<Row>,
    pub cells: Vec<CellRow>,
}

impl Mixes {
    pub fn of(screen: &Screen) -> Result<Self, String> {
        let (runs, report) = screen.runs.as_ref().map_err(Clone::clone)?;
        let mut spend: BTreeMap<String, Spend> = BTreeMap::new();
        for run in runs {
            let entry = spend.entry(run.mix()).or_insert_with(|| Spend { mix: run.mix(), runs: 0, usd: 0.0 });
            entry.runs += 1;
            entry.usd += run.usd;
        }
        let mut models: BTreeMap<String, Spend> = BTreeMap::new();
        for run in runs {
            let model = if run.model.is_empty() { "unknown" } else { run.model.as_str() };
            let entry = models.entry(model.to_owned()).or_insert_with(|| Spend { mix: model.to_owned(), runs: 0, usd: 0.0 });
            entry.runs += 1;
            entry.usd += run.usd;
        }
        let mut spend: Vec<Spend> = spend.into_values().collect();
        spend.sort_by(|a, b| b.usd.total_cmp(&a.usd));
        let mut models: Vec<Spend> = models.into_values().collect();
        models.sort_by(|a, b| b.usd.total_cmp(&a.usd));
        let mut rows: BTreeMap<(String, String), Row> = BTreeMap::new();
        let mut tiers: HashMap<&str, &str> = HashMap::new();
        for run in runs.iter().filter(|run| run.role == "worker") {
            tiers.insert(&run.agent, &run.tier);
            let row = entry(&mut rows, &run.category, &run.tier);
            row.runs += 1;
            row.usd += run.usd;
            *row.mixes.entry(run.mix()).or_default() += 1;
        }
        for cell in &report.cells {
            let row = entry(&mut rows, &cell.category, &cell.tier);
            row.closed = cell.closed;
            row.reworked = cell.reworked;
            row.usd_per_closed = Some(cell.usd_per_closed());
        }
        if let Ok(attempts) = &screen.attempts {
            for attempt in attempts {
                let Some(tier) = tiers.get(attempt.agent.as_str()) else { continue };
                let row = entry(&mut rows, &attempt.category, tier);
                row.first_tries += 1;
                row.first_tries_landed += usize::from(attempt.succeeded);
            }
        }
        for row in rows.values_mut() {
            row.usd_per_first_try_landed = (row.first_tries_landed > 0).then_some(row.usd / row.first_tries_landed as f64);
        }
        let mut rows: Vec<Row> = rows.into_values().collect();
        for row in &mut rows {
            row.mix = row.mixes.iter().max_by_key(|(_, count)| **count).map_or_else(|| row.tier.replace('-', " "), |(mix, _)| mix.clone());
        }
        let mut categories: BTreeMap<String, f64> = BTreeMap::new();
        for row in &rows {
            *categories.entry(row.category.clone()).or_default() += row.usd;
        }
        for category in categories.keys() {
            let priced: Vec<usize> = rows.iter().enumerate().filter(|(_, row)| &row.category == category && row.usd_per_closed.is_some()).map(|(index, _)| index).collect();
            if priced.len() > 1
                && let Some(&most_per_box) = priced.iter().max_by(|a, b| rows[**a].usd_per_closed.unwrap_or(0.0).total_cmp(&rows[**b].usd_per_closed.unwrap_or(0.0)))
            {
                rows[most_per_box].most_per_box = true;
            }
        }
        rows.sort_by(|a, b| categories[&b.category].total_cmp(&categories[&a.category]).then_with(|| a.category.cmp(&b.category)).then_with(|| b.runs.cmp(&a.runs)));
        let cells = cell_rows(runs, &screen.attempts, report);
        Ok(Self { spend, models, rows, cells })
    }
}

fn cell_rows(runs: &[crate::ledger::WindowRun], attempts: &Result<Vec<crate::cli::scoreboard::workers::Attempt>, String>, report: &crate::ledger::ClosedReport) -> Vec<CellRow> {
    let mut counted: BTreeMap<(String, String), (usize, usize, usize)> = BTreeMap::new();
    let mut agents: HashMap<&str, (String, String)> = HashMap::new();
    for run in runs.iter().filter(|run| run.role == "worker") {
        let Some(cell) = cells::named(&run.model, &run.effort) else { continue };
        agents.insert(&run.agent, (run.category.clone(), cell.clone()));
        counted.entry((run.category.clone(), cell)).or_default().0 += 1;
    }
    if let Ok(attempts) = attempts {
        for attempt in attempts {
            let Some((category, cell)) = agents.get(attempt.agent.as_str()) else { continue };
            let entry = counted.entry((category.clone(), cell.clone())).or_default();
            entry.1 += 1;
            entry.2 += usize::from(attempt.succeeded);
        }
    }
    let categories: std::collections::BTreeSet<&str> = counted.keys().map(|(category, _)| category.as_str()).collect();
    let mut rows = Vec::new();
    for category in categories {
        let outcomes = cells::outcomes(report, category);
        let live = cells::live(category, seed_cell(category), &outcomes);
        let (shares, _) = cells::shares(&live, &outcomes, category);
        let current: BTreeMap<String, (&str, f64)> = live.iter().zip(&shares).map(|(live, share)| (live.cell.name(), (live.role.name(), *share))).collect();
        for ((cell_category, cell), (runs, first_tries, landed)) in counted.iter().filter(|((cell_category, _), _)| cell_category == category) {
            let (role, share) = current.get(cell).copied().unwrap_or(("-", 0.0));
            rows.push(CellRow { category: cell_category.clone(), cell: cell.clone(), role: role.to_owned(), runs: *runs, first_tries: *first_tries, first_tries_landed: *landed, share });
        }
        for (at, live) in live.iter().enumerate() {
            let cell = live.cell.name();
            if counted.contains_key(&(category.to_owned(), cell.clone())) {
                continue;
            }
            rows.push(CellRow { category: category.to_owned(), cell, role: live.role.name().to_owned(), runs: 0, first_tries: 0, first_tries_landed: 0, share: shares[at] });
        }
    }
    rows.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| (a.role != "leader").cmp(&(b.role != "leader"))).then_with(|| a.cell.cmp(&b.cell)));
    rows
}

fn entry<'a>(rows: &'a mut BTreeMap<(String, String), Row>, category: &str, tier: &str) -> &'a mut Row {
    rows.entry((category.to_owned(), tier.to_owned())).or_insert_with(|| Row { category: category.to_owned(), tier: tier.to_owned(), ..Row::default() })
}
