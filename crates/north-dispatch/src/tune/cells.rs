//! The cells a tick decides from the ledger: each category's Opus effort once its effort arms decided, and the ETA and budget of each tier family and category with a closed box, each with its evidence.
use crate::ledger::ClosedReport;
use crate::dispatch::{ARM_WINDOW, BUDGET_TIMES, Effort, Evidence, LEDGER_WINDOW, Tier, eta_minutes};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Every cell's current value by name, as the cells file holds it.
pub type Cells = BTreeMap<String, Value>;

/// What a cell's value rests on: the ledger rows read, the closed boxes behind it, the 95% interval
/// of the figure it comes from, and the figures in words.
#[derive(Clone, Debug, PartialEq)]
pub struct Basis {
    pub rows: usize,
    pub n: usize,
    pub interval: Option<(f64, f64)>,
    pub why: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Decided {
    pub cell: String,
    pub value: Value,
    pub basis: Basis,
}

const FAMILIES: [Tier; 3] = [Tier::Haiku, Tier::Sonnet, Tier::Opus];

/// Decides the cells; `times` holds each category's budget multiplier and its basis where the
/// stored Budgets' p90 set one, every other category taking `BUDGET_TIMES`.
pub fn decide(week: &ClosedReport, evidence: &BTreeMap<String, Evidence>, times: &BTreeMap<String, (f64, String)>) -> Vec<Decided> {
    let mut decided: Vec<Decided> = evidence.iter().filter_map(|(category, evidence)| effort(category, evidence)).collect();
    let families: BTreeSet<(Tier, &str)> = week
        .cells
        .iter()
        .filter_map(|cell| Some((FAMILIES.into_iter().find(|tier| cell.tier.contains(tier.family()))?, cell.category.as_str())))
        .collect();
    for (tier, category) in families {
        let Some(figures) = week.figures(tier.family(), category) else { continue };
        let rows = figures.runs;
        let (low, high) = figures.minutes_interval;
        decided.push(Decided {
            cell: format!("eta_minutes {} {category}", tier.family()),
            value: json!(eta_minutes(&figures)),
            basis: Basis { rows, n: figures.closed, interval: Some((low, high)), why: format!("p75 run wall {:.1} min of n={rows} runs", figures.p75_minutes) },
        });
        let (low, high) = figures.usd_interval;
        let (multiplier, source) = times.get(category).map_or_else(|| (BUDGET_TIMES, "the constant".to_owned()), |(times, source)| (*times, source.clone()));
        decided.push(Decided {
            cell: format!("budget_usd {} {category}", tier.family()),
            value: json!(round_cents(figures.median_usd * multiplier)),
            basis: Basis {
                rows,
                n: figures.closed,
                interval: Some((round_cents(low * multiplier), round_cents(high * multiplier))),
                why: format!("{multiplier}x ({source}) the median ${:.2} per closed box of n={} boxes", figures.median_usd, figures.closed),
            },
        });
    }
    decided
}

pub const MAX_STEP: f64 = 2.0;

pub fn limit_steps(prior: &Cells, decided: &mut [Decided]) {
    for cell in decided.iter_mut() {
        let eta = cell.cell.starts_with("eta_minutes ");
        if !eta && !cell.cell.starts_with("budget_usd ") {
            continue;
        }
        let (Some(was), Some(figure)) = (prior.get(&cell.cell).and_then(Value::as_f64).filter(|was| *was > 0.0), cell.value.as_f64()) else { continue };
        let (low, high) = (was / MAX_STEP, was * MAX_STEP);
        if (low..=high).contains(&figure) {
            continue;
        }
        let held = if figure < low { low } else { high };
        let held = if eta { held.ceil() } else { round_cents(held) };
        cell.basis.why = format!("{}; held to {held} by the {MAX_STEP}x step limit on the prior {was}", cell.basis.why);
        cell.value = json!(held);
    }
}

fn effort(category: &str, evidence: &Evidence) -> Option<Decided> {
    let effort = evidence.settled_effort()?;
    let kept = |effort: Effort| evidence.efforts.closed(effort);
    Some(Decided {
        cell: format!("effort {category}"),
        value: json!(effort.name()),
        basis: Basis {
            rows: evidence.effort_runs(),
            n: Effort::ARMS.into_iter().map(kept).sum(),
            interval: None,
            why: format!("lower mean cost per kept Opus box over {ARM_WINDOW}: {}", evidence.effort_figures()),
        },
    })
}

fn round_cents(usd: f64) -> f64 {
    (usd * 100.0).round() / 100.0
}

/// One cell whose value differs from the cells file's: a new or changed value with its evidence,
/// or a cell the window no longer decides.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub cell: String,
    pub from: Option<Value>,
    pub to: Option<Value>,
    pub basis: Basis,
}

pub fn changes(prior: &Cells, decided: &[Decided]) -> Vec<Change> {
    let mut found: Vec<Change> = decided
        .iter()
        .filter(|decided| prior.get(&decided.cell) != Some(&decided.value))
        .map(|decided| Change { cell: decided.cell.clone(), from: prior.get(&decided.cell).cloned(), to: Some(decided.value.clone()), basis: decided.basis.clone() })
        .collect();
    let kept: BTreeSet<&str> = decided.iter().map(|decided| decided.cell.as_str()).collect();
    found.extend(prior.iter().filter(|(cell, _)| !kept.contains(cell.as_str())).map(|(cell, value)| Change {
        cell: cell.clone(),
        from: Some(value.clone()),
        to: None,
        basis: Basis { rows: 0, n: 0, interval: None, why: format!("no longer decided by the last {LEDGER_WINDOW} of the ledger") },
    }));
    found
}

pub fn cells(decided: &[Decided]) -> Cells {
    decided.iter().map(|decided| (decided.cell.clone(), decided.value.clone())).collect()
}

impl Change {
    pub fn line(&self) -> String {
        let shown = |value: &Option<Value>| value.as_ref().map_or_else(|| "-".to_owned(), Value::to_string);
        let interval = self.basis.interval.map_or_else(|| "none".to_owned(), |(low, high)| format!("[{low}, {high}]"));
        format!(
            "changed {}: {} -> {} (rows {}, n {}, interval {interval}; {})",
            self.cell,
            shown(&self.from),
            shown(&self.to),
            self.basis.rows,
            self.basis.n,
            self.basis.why
        )
    }

    /// The provenance record for this change, in north#40's shape: claim, origin, pointer to the
    /// rerun command, the revisit trigger, and the evidence.
    pub fn record(&self, at: &str) -> Value {
        json!({
            "at": at,
            "cell": self.cell,
            "from": self.from,
            "to": self.to,
            "rows": self.basis.rows,
            "n": self.basis.n,
            "interval": self.basis.interval.map(|(low, high)| [low, high]),
            "claim": self.basis.why,
            "origin": "experiment",
            "pointer": format!("north ledger summary --since {LEDGER_WINDOW}"),
            "revisit": "the next north tune tick",
        })
    }
}

/// Keeps each prior effort cell the tick could not decide because the arms' evidence
/// went unread, so a failed read neither drops nor re-adds an effort.
pub fn carry_efforts(prior: &Cells, decided: &mut Vec<Decided>) {
    let held: BTreeSet<String> = decided.iter().map(|decided| decided.cell.clone()).collect();
    decided.extend(prior.iter().filter(|(cell, _)| cell.starts_with("effort ") && !held.contains(*cell)).map(|(cell, value)| Decided {
        cell: cell.clone(),
        value: value.clone(),
        basis: Basis { rows: 0, n: 0, interval: None, why: "kept: the arms' evidence was unread this tick".into() },
    }));
}
