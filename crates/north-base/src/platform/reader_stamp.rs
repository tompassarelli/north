use std::fs;
use std::path::{Path, PathBuf};

pub const ROOT: &str = "north/reader-stamps";

pub fn path(state: &Path, signal: &str) -> PathBuf {
    state.join(ROOT).join(signal)
}

pub fn touch(state: &Path, signal: &str) {
    let stamp = path(state, signal);
    if let Some(parent) = stamp.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(stamp, b"");
    }
}
