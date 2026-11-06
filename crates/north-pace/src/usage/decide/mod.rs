//! Account usage decisions as pure functions of a recorded `World`: they read no clock, file or environment.
mod assess;
mod spend;
mod state;
#[cfg(test)]
mod tests;
mod view;
mod work;
pub mod world;

pub use assess::{AllOut, Assessments, assess};
pub use spend::{Assessment, Scope, Spend};
pub use state::{State, state};
pub use view::{FRESH_FOR, StaleWhy, View, view};
pub use work::Action;

pub const LEAD_FAILOVER_AFTER: world::Seconds = world::Seconds(600);
pub const PROXY_WEEK_CEILING: world::Percent = world::Percent(95.0);
