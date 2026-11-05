use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

pub fn command_v(tool: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|directory| directory.join(tool)).find(|candidate| candidate.is_file())
}

pub fn require(tool: &str) -> PathBuf {
    command_v(tool).unwrap_or_else(|| panic!("{tool} is not on PATH"))
}

pub fn minimal_path(directory: &Path, tools: &[&str]) -> PathBuf {
    std::fs::create_dir_all(directory).unwrap();
    for tool in tools {
        symlink(require(tool), directory.join(tool)).unwrap();
    }
    directory.to_path_buf()
}
