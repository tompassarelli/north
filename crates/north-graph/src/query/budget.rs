use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::error::QueryError;

pub const STEP_BUDGET: u64 = 10_000_000;
pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;
pub const MAX_TIMEOUT_MS: u64 = 60_000;
const CLOCK_EVERY: u64 = 1024;

pub struct Budget {
    steps: Cell<u64>,
    limit: u64,
    deadline: Instant,
    milliseconds: u64,
}

impl Budget {
    pub fn new(requested_ms: Option<u64>) -> Self {
        let milliseconds = requested_ms.unwrap_or(DEFAULT_TIMEOUT_MS).min(MAX_TIMEOUT_MS);
        Self { steps: Cell::new(0), limit: STEP_BUDGET, deadline: Instant::now() + Duration::from_millis(milliseconds), milliseconds }
    }

    pub fn with_step_limit(mut self, limit: u64) -> Self {
        self.limit = limit;
        self
    }

    pub fn steps(&self) -> u64 {
        self.steps.get()
    }

    pub fn tick(&self) -> Result<(), QueryError> {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        if steps > self.limit {
            return Err(QueryError::StepBudgetExceeded { steps: self.limit });
        }
        if steps.is_multiple_of(CLOCK_EVERY) && Instant::now() >= self.deadline {
            return Err(QueryError::Timeout { milliseconds: self.milliseconds });
        }
        Ok(())
    }
}
