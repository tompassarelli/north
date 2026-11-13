//! The expected spend E a brief's `Budget:` line states, kept on the session record so budget notices
//! compare what a session has spent with what its brief said. Dollars are kept in cents so the record stays `Eq`.
use serde::{Deserialize, Serialize};

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

