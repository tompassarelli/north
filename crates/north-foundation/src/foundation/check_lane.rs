use std::path::{Path, PathBuf};

pub const SLUG: &str = "done-when";

pub fn of(project: &Path) -> PathBuf {
    project.join("worktrees").join(SLUG)
}
