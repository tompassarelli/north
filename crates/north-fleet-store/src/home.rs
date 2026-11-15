use std::path::{Path, PathBuf};

use north_foundation::error::{NorthError, NorthResult};

pub fn home_dir() -> NorthResult<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is not set".into()))
}

pub fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/north")
}

pub fn live_state_dir(home: &Path) -> PathBuf {
    state_in(home, std::env::var_os("XDG_STATE_HOME").as_deref())
}

fn state_in(home: &Path, xdg: Option<&std::ffi::OsStr>) -> PathBuf {
    xdg.filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map_or_else(|| state_dir(home), |path| path.join("north"))
}

pub fn fleet_root(state: &Path) -> PathBuf {
    state.join("fleet")
}

pub fn root_here() -> NorthResult<PathBuf> {
    Ok(fleet_root(&live_state_dir(&home_dir()?)))
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
