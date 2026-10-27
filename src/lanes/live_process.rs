use std::fs;
use std::path::Path;

pub(super) fn has_cwd_inside(proc_root: &Path, tree: &Path) -> bool {
    let Ok(entries) = fs::read_dir(proc_root) else {
        return false;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()))
        .any(|entry| fs::read_link(entry.path().join("cwd")).is_ok_and(|target| target.starts_with(tree)))
}
