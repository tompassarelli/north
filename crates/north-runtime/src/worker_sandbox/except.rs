use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// Which entries become rules: removal is a right of the directory that holds a file, so its rules are directories; reading is a right of the file itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Entries {
    Directories,
    All,
}

type Identity = (u64, u64);

fn identity(path: &Path) -> Option<Identity> {
    std::fs::metadata(path).ok().map(|metadata| (metadata.dev(), metadata.ino()))
}

/// The entries under `root` other than the `protected` paths. A Landlock rule holds an inode, and one inode can sit at several paths (two mounts of one btrfs subvolume at `/home` and `/nix` make `/nix/USER` the same directory as `/home/USER`), so entries are matched by device and inode: an entry that is a protected path is left out, one that holds a protected path is walked instead of granted, each such directory once, and a symlink is followed only toward a protected path and never granted itself (`~/.local/state/north` links to `~/code/north-data`, which holds the profiles).
pub fn beneath_except(root: &Path, protected: &[PathBuf], entries: Entries) -> Vec<PathBuf> {
    let real: Vec<PathBuf> = protected.iter().filter_map(|path| path.canonicalize().ok()).collect();
    let hidden: HashSet<Identity> = real.iter().filter_map(|path| identity(path)).collect();
    let holders: HashSet<Identity> = real.iter().flat_map(|path| path.ancestors().skip(1)).filter_map(identity).collect();
    let mut walk = Walk { hidden, holders, entries, visited: HashSet::new(), rules: Vec::new() };
    walk.directory(root);
    walk.rules
}

struct Walk {
    hidden: HashSet<Identity>,
    holders: HashSet<Identity>,
    entries: Entries,
    visited: HashSet<Identity>,
    rules: Vec<PathBuf>,
}

impl Walk {
    fn directory(&mut self, directory: &Path) {
        if !identity(directory).is_some_and(|own| self.visited.insert(own)) {
            return;
        }
        for path in std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()) {
            let Ok(metadata) = std::fs::metadata(&path) else {
                continue;
            };
            let own = (metadata.dev(), metadata.ino());
            if self.hidden.contains(&own) || (self.entries == Entries::Directories && !metadata.is_dir()) {
                continue;
            }
            if self.holders.contains(&own) {
                self.directory(&path);
            } else if !path.is_symlink() {
                self.rules.push(path);
            }
        }
    }
}
