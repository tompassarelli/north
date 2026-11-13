//! The snapshot and its sidecars share one path: the partial, tried and served files sit beside it with a
//! suffix, and so does the temporary a publish renames into place.
use std::path::{Path, PathBuf};

pub(super) fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
