//! The dispatch and spend figures of the window: per task category and model mix, its runs, first tries and the
//! cost of a closed box, and the spend of every role by model and effort.
use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

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
    pub closed: usize,
    pub usd_per_closed: Option<f64>,
    /// The mix whose closed box costs most in a category where more than one mix closed a box.
    pub dearest: bool,
    #[serde(skip)]
    mixes: BTreeMap<String, usize>,
}

pub struct Mixes {
    pub spend: Vec<Spend>,
    pub models: Vec<Spend>,
    pub rows: Vec<Row>,
    /// The distinct boxes closed in the window, each counted once though several mixes worked it.
    pub closed: usize,
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
                && let Some(&dearest) = priced.iter().max_by(|a, b| rows[**a].usd_per_closed.unwrap_or(0.0).total_cmp(&rows[**b].usd_per_closed.unwrap_or(0.0)))
            {
                rows[dearest].dearest = true;
            }
        }
        rows.sort_by(|a, b| categories[&b.category].total_cmp(&categories[&a.category]).then_with(|| a.category.cmp(&b.category)).then_with(|| b.runs.cmp(&a.runs)));
        Ok(Self { spend, models, rows, closed: report.closed })
    }
}

fn entry<'a>(rows: &'a mut BTreeMap<(String, String), Row>, category: &str, tier: &str) -> &'a mut Row {
    rows.entry((category.to_owned(), tier.to_owned())).or_insert_with(|| Row { category: category.to_owned(), tier: tier.to_owned(), ..Row::default() })
}
