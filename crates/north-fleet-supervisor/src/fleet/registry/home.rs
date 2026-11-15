use std::collections::BTreeSet;

use crate::error::NorthResult;

use super::super::prompt::StandingOrders;
use super::super::records::{RetiredReport, recorded_names, retired_report};
use super::super::roster::Roster;
use super::super::runtimes::Runtimes;

pub(super) use super::super::home::{fleet_root, home_dir, live_state_dir, state_dir};

pub fn roster(runtimes: &Runtimes) -> NorthResult<Roster> {
    let home = home_dir()?;
    Ok(runtimes.roster(&state_dir(&home), &home, &crate::platform::orchestration::path()))
}

/// This host's standing orders, for briefs that start outside a fleet launch.
pub fn orders_here() -> NorthResult<StandingOrders> {
    Ok(StandingOrders::in_state(&live_state_dir(&home_dir()?)))
}

/// The names of every session this host's fleet has run, live or past.
pub fn recorded_here() -> NorthResult<BTreeSet<String>> {
    Ok(recorded_names(&fleet_root(&live_state_dir(&home_dir()?))))
}

/// The final report a retired session of this host's fleet left in its archive.
pub fn retired_report_here(name: &str) -> Option<RetiredReport> {
    retired_report(&fleet_root(&live_state_dir(&home_dir().ok()?)), name)
}
