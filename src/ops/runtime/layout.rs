use std::path::{Path, PathBuf};

pub(crate) const PROGRAMS: [&str; 2] = ["north", "verify-north"];
pub(crate) use crate::platform::harness::{TREE, is_sha};
const KEPT_OLDER: usize = 3;

pub(crate) struct Layout {
    pub(crate) state: PathBuf,
}

impl Layout {
    pub(crate) fn from_env() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/state"));
        Self { state: state.join("north") }
    }

    pub(crate) fn runtimes(&self) -> PathBuf {
        self.state.join("runtimes")
    }

    pub(crate) fn current(&self) -> PathBuf {
        self.state.join("current")
    }

    pub(crate) fn runtime(&self, sha: &str) -> PathBuf {
        self.runtimes().join(sha)
    }
}

pub(crate) fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

pub(crate) fn link(sha: &str) -> PathBuf {
    Path::new("runtimes").join(sha)
}

pub(crate) fn linked_sha(link: &Path) -> Option<&str> {
    link.strip_prefix("runtimes").ok()?.to_str().filter(|name| is_sha(name))
}

pub(crate) fn has_programs(runtime: &Path) -> bool {
    PROGRAMS.iter().all(|program| runtime.join(program).is_file())
}

pub(crate) fn doomed<'a>(newest_first: &'a [String], keep: &[&str]) -> Vec<&'a str> {
    newest_first
        .iter()
        .map(String::as_str)
        .filter(|name| is_sha(name) && !keep.contains(name))
        .skip(KEPT_OLDER)
        .collect()
}
