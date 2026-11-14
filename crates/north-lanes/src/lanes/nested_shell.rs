use std::path::Path;

use super::live_process;
use super::repo::{git_checked, git_out};

pub(super) fn real_main(container: &Path) -> Option<&Path> {
    let main = container.parent().filter(|parent| parent.file_name().is_some_and(|name| name == "main") && parent.join(".git").exists())?;
    (main.parent()?.file_name() == container.file_name()).then_some(main)
}

pub(super) fn remove(main: &Path, lane: &Path, proc_root: &Path) -> Result<String, String> {
    let listing = git_out(main, &["worktree", "list", "--porcelain"]).unwrap_or_default();
    if !listing.lines().filter_map(|line| line.strip_prefix("worktree ")).any(|path| Path::new(path) == lane) {
        return Err("a nested lane git does not register".to_owned());
    }
    let mut entries = std::fs::read_dir(lane).map_err(|error| error.to_string())?.flatten().map(|entry| entry.file_name());
    if entries.next().is_none_or(|name| name != ".git") || entries.next().is_some() {
        return Err(format!("it sits inside the main checkout and holds files; move it with `git -C {} worktree move {} /home/tom/code/PROJECT/worktrees/NAME`", main.display(), lane.display()));
    }
    if live_process::has_cwd_inside(proc_root, lane) {
        return Err("a live process has its cwd inside".to_owned());
    }
    let lane_text = lane.to_string_lossy();
    let _ = git_checked(main, &["worktree", "unlock", &lane_text]);
    std::fs::remove_file(lane.join(".git")).map_err(|error| format!("cannot delete its .git file: {error}"))?;
    std::fs::remove_dir(lane).map_err(|error| format!("cannot delete it: {error}"))?;
    git_checked(main, &["worktree", "prune"])?;
    let worktrees = lane.parent();
    let container = worktrees.and_then(Path::parent);
    for empty in [worktrees, container].into_iter().flatten() {
        let _ = std::fs::remove_dir(empty);
    }
    Ok(format!("removed: {} (an empty shell nested inside the main checkout)", lane.display()))
}
