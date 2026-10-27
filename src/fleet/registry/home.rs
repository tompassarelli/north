use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

use super::super::prompt::StandingOrders;
use super::super::records::recorded_names;
use super::super::roster::Roster;
use super::super::runtimes::Runtimes;

pub(super) fn home_dir() -> NorthResult<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is not set".into()))
}

pub fn roster(runtimes: &Runtimes) -> NorthResult<Roster> {
    let home = home_dir()?;
    Ok(runtimes.roster(&state_dir(&home), &home, &crate::pace::target::policy_path()))
}

pub(super) fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/north")
}

/// This host's standing orders, for briefs that start outside a fleet launch.
pub fn orders_here() -> NorthResult<StandingOrders> {
    Ok(StandingOrders::in_state(&state_dir(&home_dir()?)))
}

/// The names of every session this host's fleet has run, live or past.
pub fn recorded_here() -> NorthResult<BTreeSet<String>> {
    Ok(recorded_names(&fleet_root(&state_dir(&home_dir()?))))
}

pub(super) fn fleet_root(state: &Path) -> PathBuf {
    state.join("fleet")
}
