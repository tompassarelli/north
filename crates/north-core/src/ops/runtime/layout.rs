use std::fs;
use std::path::{Path, PathBuf};

pub const PROGRAMS: [&str; 2] = ["north", "verify-north"];
pub use crate::platform::harness::{TREE, is_sha};
const KEPT_OLDER: usize = 3;

pub struct Layout {
    pub state: PathBuf,
}

impl Layout {
    pub fn from_env() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/state"));
        Self { state: state.join("north") }
    }

    pub fn runtimes(&self) -> PathBuf {
        self.state.join("runtimes")
    }

    pub fn current(&self) -> PathBuf {
        self.state.join("current")
    }

    pub fn failed(&self) -> PathBuf {
        self.state.join("runtime-failed")
    }

    pub fn build_seconds(&self) -> PathBuf {
        self.state.join("runtime-build-seconds")
    }

    pub fn runtime(&self, sha: &str) -> PathBuf {
        self.runtimes().join(sha)
    }
}

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub fn short(sha: &str) -> &str {
    sha.get(..8).unwrap_or(sha)
}

pub fn link(sha: &str) -> PathBuf {
    Path::new("runtimes").join(sha)
}

pub fn linked_sha(link: &Path) -> Option<&str> {
    link.strip_prefix("runtimes").ok()?.to_str().filter(|name| is_sha(name))
}

pub fn has_programs(runtime: &Path) -> bool {
    PROGRAMS.iter().all(|program| runtime.join(program).is_file())
}

pub fn running(proc_root: &Path, runtimes: &Path) -> Vec<String> {
    let runtimes = fs::canonicalize(runtimes).unwrap_or_else(|_| runtimes.to_path_buf());
    let mut names = fs::read_dir(proc_root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|process| fs::read_link(process.path().join("exe")).ok())
        .filter_map(|exe| exe.strip_prefix(&runtimes).ok()?.components().next()?.as_os_str().to_str().map(str::to_owned))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

pub fn doomed<'a>(newest_first: &'a [String], keep: &[&str]) -> Vec<&'a str> {
    newest_first
        .iter()
        .map(String::as_str)
        .filter(|name| is_sha(name) && !keep.contains(name))
        .skip(KEPT_OLDER)
        .collect()
}
