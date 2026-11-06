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
