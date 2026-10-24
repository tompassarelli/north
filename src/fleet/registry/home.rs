use std::path::{Path, PathBuf};

use crate::error::{NorthError, NorthResult};

use super::super::roster::Roster;
use super::super::runtime::Runtimes;

pub(super) fn home_dir() -> NorthResult<PathBuf> {
    std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| NorthError::Configuration("HOME is not set".into()))
}

pub fn roster(runtimes: &Runtimes) -> NorthResult<Roster> {
    let home = home_dir()?;
    Ok(runtimes.roster(&state_dir(&home), &home))
}

pub(super) fn state_dir(home: &Path) -> PathBuf {
    home.join(".local/state/north")
}
