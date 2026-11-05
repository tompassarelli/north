//! Account usage decisions as pure functions of a recorded `World`: they read no clock, file or environment.
mod assess;
mod explain;
mod free;
mod place;
mod rank;
mod spend;
mod state;
#[cfg(test)]
mod tests;
mod view;
mod work;
pub mod world;

pub use assess::{AllOut, Assessments, assess};
pub use explain::explain;
pub use free::{Free, Gate, Shortfall, free};
pub use place::{Decision, Note, Refusal, place};
pub use rank::{Block, Entry, Ranked, Tier, Verdict, Warning, rank};
pub use spend::{Assessment, Scope, Spend};
pub use state::{State, state};
pub use view::{FRESH_FOR, StaleWhy, View, view};
pub use work::Action;

pub const LEAD_FAILOVER_AFTER: world::Seconds = world::Seconds(600);
