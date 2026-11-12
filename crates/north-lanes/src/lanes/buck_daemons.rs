//! The Buck2 daemon state of checkouts that are gone. Buck2 keeps one daemon per project root and isolation dir, with
//! its state at `~/.buck/buckd/<project root>/<isolation dir>`, and a daemon stops by itself within a minute of losing
//! that state, so dropping the state of a removed lane is what stops its daemon, however the lane was removed
//! (docs/rationale/compile-reuse.md, Lanes).
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use north_base::platform::shared_target;

pub struct DeadDaemon {
    pub path: PathBuf,
    pub bytes: u64,
}

/// Removes, under `buckd`, the state mirroring every path that is no directory under `root` any more, and returns what it removed (or would, on a dry run). The state of a checkout that still holds a `.buckconfig` is left whole, whatever its isolation dirs are named.
pub fn reclaim_dead_daemons(buckd: &Path, root: &Path, dry_run: bool) -> Vec<DeadDaemon> {
    let mut dead = Vec::new();
    if buckd.is_absolute() {
        gone(buckd, root, Path::new(""), &mut dead);
    }
    dead.sort();
    dead.into_iter()
        .map(|path| DeadDaemon { bytes: shared_target::allocated_tree(&path, &mut HashSet::new()).map_or(0, |(bytes, _)| bytes), path })
        .filter(|daemon| dry_run || fs::remove_dir_all(&daemon.path).is_ok())
        .collect()
}

fn gone(buckd: &Path, root: &Path, mirrored: &Path, dead: &mut Vec<PathBuf>) {
    if root.join(mirrored).join(".buckconfig").is_file() {
        return;
    }
    let children = fs::read_dir(buckd.join(mirrored)).into_iter().flatten().flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()));
    for child in children {
        let mirrored = mirrored.join(child.file_name());
        if root.join(&mirrored).is_dir() { gone(buckd, root, &mirrored, dead) } else { dead.push(child.path()) }
    }
}

#[cfg(test)]
#[path = "buck_daemons_tests.rs"]
mod tests;
