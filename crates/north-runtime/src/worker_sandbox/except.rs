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

/// The entries under `root` other than the `protected` paths. A Landlock rule holds an inode, and one inode can sit at several paths (two mounts of one btrfs subvolume at `/home` and `/nix` make `/nix/USER` the same directory as `/home/USER`), so entries are matched by device and inode: an entry that is a protected path is left out, one that holds a protected path is walked instead of granted, each such directory once, a protected path that does not exist yet keeps its directory walked so it stays protected once created, and a symlink is followed only toward a protected path and never granted itself (`~/.local/state/north` links to `~/code/north-data`, which holds the profiles), and a directory this user may enter but not list is walked through the entries on the way to a protected path, the only ones it can name.
pub fn beneath_except(root: &Path, protected: &[PathBuf], entries: Entries) -> Vec<PathBuf> {
    let real: Vec<PathBuf> = protected.iter().filter_map(|path| path.canonicalize().ok().or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))).collect();
    let hidden: HashSet<Identity> = real.iter().filter_map(|path| identity(path)).collect();
    let holders: HashSet<Identity> = real.iter().flat_map(|path| path.ancestors().skip(1)).filter_map(identity).collect();
    let mut walk = Walk { hidden, holders, entries, visited: HashSet::new(), rules: Vec::new(), real };
    walk.directory(root);
    walk.rules
}

struct Walk {
    hidden: HashSet<Identity>,
    holders: HashSet<Identity>,
    entries: Entries,
    visited: HashSet<Identity>,
    rules: Vec<PathBuf>,
    real: Vec<PathBuf>,
}

impl Walk {
    fn directory(&mut self, directory: &Path) {
        if !identity(directory).is_some_and(|own| self.visited.insert(own)) {
            return;
        }
        let listed: Option<Vec<PathBuf>> = std::fs::read_dir(directory).ok().map(|read| read.flatten().map(|entry| entry.path()).collect());
        for path in listed.unwrap_or_else(|| self.toward_protected(directory)) {
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

    fn toward_protected(&self, directory: &Path) -> Vec<PathBuf> {
        let mut named: Vec<PathBuf> = self.real.iter().filter_map(|path| path.strip_prefix(directory).ok()?.components().next().map(|first| directory.join(first))).collect();
        named.sort();
        named.dedup();
        named
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn a_directory_this_user_can_enter_but_not_list_is_walked_toward_its_protected_path_and_grants_only_what_lies_on_the_way() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        let shut = root.join("shut");
        let lane = shut.join("held/lane");
        let secret = shut.join("held/secret");
        std::fs::create_dir_all(&lane).unwrap();
        std::fs::create_dir_all(shut.join("unnamed")).unwrap();
        std::fs::write(&secret, "").unwrap();
        let open = std::fs::metadata(&shut).unwrap().permissions();
        std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o300)).unwrap();
        let rules = beneath_except(&root, std::slice::from_ref(&secret), Entries::All);
        std::fs::set_permissions(&shut, open).unwrap();
        assert_eq!(rules, [lane], "the lane beside the protected file is granted, the protected file and the unlistable directory's unnamed entries are not");
    }
}
