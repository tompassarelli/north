use std::fs;
use std::path::{Path, PathBuf};

pub fn pid_path(dir: &Path) -> PathBuf {
    dir.join("host.pid")
}

pub fn scope_path(dir: &Path) -> PathBuf {
    dir.join("host.scope")
}

pub fn pid(dir: &Path) -> Option<u32> {
    fs::read_to_string(pid_path(dir)).ok()?.trim().parse().ok()
}

pub fn scope(dir: &Path) -> Option<String> {
    fs::read_to_string(scope_path(dir)).ok().map(|unit| unit.trim().to_owned()).filter(|unit| !unit.is_empty())
}
