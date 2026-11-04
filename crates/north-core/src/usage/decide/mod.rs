//! Account usage decisions as pure functions of a recorded `World`: they read no clock, file or environment.
mod state;
#[cfg(test)]
mod tests;
mod view;
pub mod world;

pub use state::{State, state};
pub use view::{FRESH_FOR, StaleWhy, View, view};

pub const LEAD_FAILOVER_AFTER: world::Seconds = world::Seconds(600);
