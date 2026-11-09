use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

use super::super::prompt::StandingOrders;
use super::super::records::{RetiredReport, recorded_names, retired_report};
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
    Ok(runtimes.roster(&state_dir(&home), &home, &crate::platform::orchestration::path()))
}

pub(super) fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/north")
}

pub(super) fn live_state_dir(home: &Path) -> PathBuf {
    state_in(home, std::env::var_os("XDG_STATE_HOME").as_deref())
}

fn state_in(home: &Path, xdg: Option<&std::ffi::OsStr>) -> PathBuf {
    xdg.filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map_or_else(|| state_dir(home), |path| path.join("north"))
}

/// This host's standing orders, for briefs that start outside a fleet launch.
pub fn orders_here() -> NorthResult<StandingOrders> {
    Ok(StandingOrders::in_state(&live_state_dir(&home_dir()?)))
}

pub fn root_here() -> NorthResult<PathBuf> {
    Ok(fleet_root(&live_state_dir(&home_dir()?)))
}

/// The names of every session this host's fleet has run, live or past.
pub fn recorded_here() -> NorthResult<BTreeSet<String>> {
    Ok(recorded_names(&fleet_root(&live_state_dir(&home_dir()?))))
}

pub(super) fn fleet_root(state: &Path) -> PathBuf {
    state.join("fleet")
}

/// The final report a retired session of this host's fleet left in its archive.
pub fn retired_report_here(name: &str) -> Option<RetiredReport> {
    retired_report(&fleet_root(&live_state_dir(&home_dir().ok()?)), name)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;

    use super::state_in;

    #[test]
    fn the_live_state_follows_an_absolute_xdg_state_home_and_ignores_a_relative_or_empty_one() {
        let home = Path::new("/home/tester");
        assert_eq!(state_in(home, None), home.join(".local/state/north"));
        assert_eq!(state_in(home, Some(OsStr::new(""))), home.join(".local/state/north"));
        assert_eq!(state_in(home, Some(OsStr::new("relative/state"))), home.join(".local/state/north"));
        assert_eq!(state_in(home, Some(OsStr::new("/fixture/state"))), Path::new("/fixture/state/north"));
    }
}
