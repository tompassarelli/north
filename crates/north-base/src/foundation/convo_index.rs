use std::path::{Path, PathBuf};

pub const FILE: &str = "index-v6.db";

pub fn under(state: &Path) -> PathBuf {
    state.join(FILE)
}

pub fn default_path(home: &Path) -> PathBuf {
    under(&home.join(".local/state/convo"))
}
