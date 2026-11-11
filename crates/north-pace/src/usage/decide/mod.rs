//! Account usage decisions as pure functions of a recorded `World`: they read no clock, file or environment.
mod assess;
mod keep;
mod state;
#[cfg(test)]
mod tests;
mod view;
mod work;
pub mod world;

pub use assess::{AllOut, Assessments, assess};
pub use keep::{API_FLOOR, SUBSCRIPTION_FLOOR};
pub use state::{State, state};
pub use view::{FRESH_FOR, StaleWhy, View, view};
pub use work::Action;
