use super::layout::TREE;
use crate::platform::entry_points::ENTRY_POINTS;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::Path;

const TARGET: &str = "../../north";

pub(super) fn link(runtime: &Path) -> io::Result<()> {
    let bin = runtime.join(TREE).join("bin");
    fs::create_dir_all(&bin)?;
    for (name, _) in ENTRY_POINTS {
        let path = bin.join(name);
        if fs::symlink_metadata(&path).is_err() {
            symlink(TARGET, &path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime_with(tracked: &[&str]) -> tempfile::TempDir {
        let runtime = tempfile::tempdir().unwrap();
        fs::write(runtime.path().join("north"), b"north").unwrap();
        fs::create_dir_all(runtime.path().join(TREE).join("bin")).unwrap();
        for name in tracked {
            fs::write(runtime.path().join(TREE).join("bin").join(name), b"#!/bin/bash\n").unwrap();
        }
        runtime
    }

    #[test]
    fn a_tree_gets_a_link_to_the_runtimes_north_once_and_a_tracked_file_at_that_path_stays() {
        let runtime = runtime_with(&["agents"]);
        link(runtime.path()).unwrap();
        let path = runtime.path().join(TREE).join("bin/safe-push");
        assert_eq!(fs::canonicalize(&path).unwrap(), fs::canonicalize(runtime.path().join("north")).unwrap());
        link(runtime.path()).unwrap();
        assert_eq!(fs::read_link(&path).unwrap(), Path::new(TARGET));

        let tracked = runtime_with(&["safe-push"]);
        link(tracked.path()).unwrap();
        assert!(fs::symlink_metadata(tracked.path().join(TREE).join("bin/safe-push")).unwrap().file_type().is_file());
    }
}
