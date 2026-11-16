//! North's off switches, a program of its own so they answer while north's runtime is missing or broken:
//! `north fleet off` stops the fleet's sessions and units on every host and records them, and `north fleet on`
//! starts exactly those. It depends on nothing of north's: the hosts' systemd user managers, `ssh` and the files
//! it records are its whole world, so a broken runtime, lander or broker cannot stop it.

mod config;
mod fleet_off;
mod fleet_on;
mod host;
mod paths;
mod record;
mod table;
mod units;

pub use fleet_off::{Mode, off};
pub use fleet_on::on;
