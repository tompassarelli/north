//! The Pi harness: the second agent program, beside OpenCode, that runs DeepSeek V4.1 Flash on the same Go plan, so the ledger can compare the two on the same boxes. It shares the lane sandbox, the visibility routing, the secret screen and the turn loop with OpenCode and differs only in how a turn is spelled and how its JSON events read.
mod events;
mod harness;
pub(crate) mod windows;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use harness::{KEY_ENV, PI, PROVIDER};

use crate::error::NorthResult;

pub fn run(arguments: &[String]) -> NorthResult<u8> {
    super::opencode::turns::run(&PI, arguments)
}
