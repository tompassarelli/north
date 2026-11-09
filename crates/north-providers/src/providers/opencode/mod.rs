mod accounts;
mod config;
mod fleet;
pub mod harness;
mod live;
pub mod route;
mod screen;
pub mod turns;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub use accounts::LABEL as ACCOUNT;
pub use fleet::{Deepseek, PROVIDER};
pub use route::{Route, Visibility};
