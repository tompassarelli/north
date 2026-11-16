//! North's off switches, a program of its own so they answer while north's runtime is missing or broken:
//! `north fleet off` stops the fleet's sessions and units on every host and records them, and `north fleet on`
//! starts exactly those; `north policy off` takes north's hooks, skills, instructions and git checks away on this host
//! and `north policy on` restores them. It depends on nothing of north's: the hosts' systemd user managers, `ssh` and the files
//! it records are its whole world, so a broken runtime, lander or broker cannot stop it.

mod config;
mod fleet_off;
mod fleet_on;
mod host;
mod paths;
mod policy;
mod policy_off;
mod policy_on;
mod record;
mod table;
mod units;

pub use fleet_off::{Mode, off};
pub use fleet_on::on;
pub use policy_off::off as policy_off;
pub use policy_on::on as policy_on;

pub fn fleet_session() -> Option<String> {
    std::env::var("NORTH_FLEET_SESSION").ok().filter(|name| !name.is_empty())
}

pub fn policy_is_off() -> bool {
    paths::Paths::here().is_ok_and(|paths| policy::Stash::new(&paths.state_home).is_off())
}
