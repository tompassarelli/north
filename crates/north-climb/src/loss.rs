//! The ledger's measured loss: what a (tier, category) cell spent on runs that did not close its box
//! the first time, and on boxes that never closed.
use north_dispatch::ledger::closed::{Cell, ClosedReport};

#[derive(Clone, Debug, PartialEq)]
pub struct Loss {
    pub tier: String,
    pub category: String,
    pub repeat_usd: f64,
    pub unclosed_usd: f64,
    pub closed: usize,
    pub runs: usize,
}

impl Loss {
    pub fn usd(&self) -> f64 {
        self.repeat_usd + self.unclosed_usd
    }
}

fn of(cell: &Cell) -> Loss {
    let repeat_usd = cell.boxes_dearest_first().iter().map(|closed| closed.usd - closed.runs.first().map_or(0.0, |first| first.usd)).sum();
    Loss { tier: cell.tier.clone(), category: cell.category.clone(), repeat_usd, unclosed_usd: cell.unclosed_usd, closed: cell.closed, runs: cell.runs }
}

/// Every routable cell's loss, the largest first; a category the ledger could not name has no route to change.
pub fn ranked(report: &ClosedReport) -> Vec<Loss> {
    let mut losses: Vec<Loss> = report.cells.iter().filter(|cell| cell.category != "unknown").map(of).collect();
    losses.sort_by(|a, b| b.usd().total_cmp(&a.usd()));
    losses
}

pub fn largest(report: &ClosedReport) -> Option<Loss> {
    ranked(report).into_iter().next().filter(|loss| loss.usd() > 0.0)
}
