mod accounts;
mod config;
mod fleet;
mod live;
mod route;
mod screen;
pub mod turns;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use accounts::LABEL as ACCOUNT;
pub use fleet::{Opencode, PROVIDER, slots};
pub use route::{Route, Visibility};
