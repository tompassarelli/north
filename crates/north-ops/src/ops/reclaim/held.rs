use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub(super) struct Held(BTreeSet<PathBuf>);

impl Held {
    pub(super) fn within(&self, root: &Path) -> bool {
        self.0.range(root.to_path_buf()..).next().is_some_and(|path| path.starts_with(root))
    }
}

pub(super) fn paths(proc_root: &Path) -> Held {
    let mut held = BTreeSet::new();
    let Ok(processes) = fs::read_dir(proc_root) else { return Held(held) };
    for process in processes.flatten() {
        if !process.file_name().to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let dir = process.path();
        held.extend(["cwd", "root"].iter().filter_map(|link| fs::read_link(dir.join(link)).ok()));
        if let Ok(descriptors) = fs::read_dir(dir.join("fd")) {
            held.extend(descriptors.flatten().filter_map(|descriptor| fs::read_link(descriptor.path()).ok()));
        }
    }
    held.extend(bound_sockets(proc_root));
    held.retain(|path| path != Path::new("/"));
    Held(held)
}

fn bound_sockets(proc_root: &Path) -> impl Iterator<Item = PathBuf> {
    let table = fs::read_to_string(proc_root.join("net/unix")).unwrap_or_default();
    table.lines().skip(1).filter_map(|line| line.split_whitespace().nth(7)).filter(|path| path.starts_with('/')).map(PathBuf::from).collect::<Vec<_>>().into_iter()
}
