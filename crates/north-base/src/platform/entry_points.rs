use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::platform::harness::TREE;

const LINK_TARGET: &str = "../../north";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryPoint {
    SafePush,
    WorkerSandbox,
}

/// The names that select an entry point from argv[0]; the runtime installer links each one to the north binary.
pub const ENTRY_POINTS: [(&str, EntryPoint); 2] = [(EntryPoint::SafePush.name(), EntryPoint::SafePush), (EntryPoint::WorkerSandbox.name(), EntryPoint::WorkerSandbox)];

/// The entry point argv[0] names, by its final path component.
pub fn named(argv0: &OsStr) -> Option<EntryPoint> {
    let name = Path::new(argv0).file_name()?.to_str()?;
    ENTRY_POINTS.iter().find(|(entry, _)| *entry == name).map(|(_, point)| *point)
}

impl EntryPoint {
    /// The file name a link to north takes to select this entry point.
    pub const fn name(self) -> &'static str {
        match self {
            Self::SafePush => "safe-push",
            Self::WorkerSandbox => "worker-sandbox",
        }
    }

    /// Variables the entry's launcher scrubbed before it ran, so a link that bypasses the launcher keeps the same environment.
    pub fn scrubbed_variables(self) -> &'static [&'static str] {
        match self {
            Self::SafePush => &["CODEX_HOME"],
            Self::WorkerSandbox => &[],
        }
    }
}


/// Links each entry point in `runtime`'s tree to the runtime's north, keeping a file the tree already tracks at that path, and links worker-sandbox beside that north.
pub fn link(runtime: &Path) -> io::Result<()> {
    linked_beside(&runtime.join("north"), EntryPoint::WorkerSandbox)?;
    let bin = runtime.join(TREE).join("bin");
    fs::create_dir_all(&bin)?;
    for (name, _) in ENTRY_POINTS {
        let path = bin.join(name);
        if fs::symlink_metadata(&path).is_err() {
            symlink(LINK_TARGET, &path)?;
        }
    }
    Ok(())
}

/// The link to `north` for `point` in north's own directory, made when absent. A candidate runtime has no tree until its install, yet its canary launches a worker first, so a launch names this link rather than one in the tree (north#1172).
pub fn linked_beside(north: &Path, point: EntryPoint) -> io::Result<PathBuf> {
    let target = north.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("{} names no file", north.display())))?;
    let path = north.with_file_name(point.name());
    match symlink(target, &path) {
        Err(error) if error.kind() != io::ErrorKind::AlreadyExists => return Err(error),
        _ => {}
    }
    if path.is_file() { Ok(path) } else { Err(io::Error::new(io::ErrorKind::NotFound, format!("{} leads to no file", path.display()))) }
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
        assert_eq!(fs::read_link(&path).unwrap(), Path::new(LINK_TARGET));

        let tracked = runtime_with(&["safe-push"]);
        link(tracked.path()).unwrap();
        assert!(fs::symlink_metadata(tracked.path().join(TREE).join("bin/safe-push")).unwrap().file_type().is_file());
    }
}
