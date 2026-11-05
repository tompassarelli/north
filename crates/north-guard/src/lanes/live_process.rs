use std::fs;
use std::path::{Path, PathBuf};

fn processes(proc_root: &Path) -> impl Iterator<Item = PathBuf> {
    fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()))
        .map(|entry| entry.path())
}

pub(super) fn has_cwd_inside(proc_root: &Path, tree: &Path) -> bool {
    processes(proc_root).any(|process| fs::read_link(process.join("cwd")).is_ok_and(|target| target.starts_with(tree)))
}

pub(super) fn open_paths(proc_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for process in processes(proc_root) {
        paths.extend(fs::read_link(process.join("cwd")));
        let descriptors = fs::read_dir(process.join("fd")).into_iter().flatten().flatten();
        paths.extend(descriptors.filter_map(|fd| fs::read_link(fd.path()).ok()).filter(|target| target.is_absolute()));
    }
    paths.sort();
    paths.dedup();
    paths
}

pub(super) fn any_inside(paths: &[PathBuf], tree: &Path) -> bool {
    paths.iter().any(|path| path.starts_with(tree))
}
