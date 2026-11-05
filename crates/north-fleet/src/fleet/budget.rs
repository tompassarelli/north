//! The expected spend E a brief's `Budget:` line states, kept on the session record so the spend breaker
//! compares what a session has spent with what its brief said. Dollars are kept in cents so the record stays `Eq`.
use serde::{Deserialize, Serialize};

use super::silent::eta_minutes;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Stated {
    pub p50_cents: Option<u64>,
    pub p90_cents: u64,
    pub minutes: u64,
}

impl Stated {
    pub fn p50(&self) -> Option<f64> {
        self.p50_cents.map(|cents| cents as f64 / 100.0)
    }

    pub fn p90(&self) -> f64 {
        self.p90_cents as f64 / 100.0
    }
}

/// `Budget: E p50 $X, p90 $Y ...` with the `ETA:` line's minutes; None when the brief states no p90.
pub fn stated(text: &str) -> Option<Stated> {
    let line = text.lines().find_map(|line| line.trim().strip_prefix("Budget:"))?;
    let p90_cents = cents_after(line, "p90")?;
    Some(Stated { p50_cents: cents_after(line, "p50"), p90_cents, minutes: eta_minutes(text).unwrap_or(0) })
}

fn cents_after(line: &str, label: &str) -> Option<u64> {
    let rest = line.split_once(&format!("{label} $"))?.1;
    let dollars: f64 = rest.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.trim_end_matches('.').parse().ok()?;
    Some((dollars * 100.0).round() as u64)
}
