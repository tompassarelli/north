//! The least GitHub rate limit a decision saw left when it spent GitHub calls.
use alloc::string::String;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    /// The rate-limit resource the lowest reading was for: `core`, `graphql`, `search`.
    pub resource: String,
    pub remaining: u64,
    /// Seconds since the epoch when that window refills.
    pub resets_at: f64,
}

impl Quota {
    /// Whether the reading is for a resource that refills hourly in thousands of calls; `search` allows 30 a minute and could never hold the floor.
    pub fn is_hourly(&self) -> bool {
        matches!(self.resource.as_str(), "core" | "graphql")
    }
}
