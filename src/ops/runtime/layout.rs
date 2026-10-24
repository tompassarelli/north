use std::path::{Path, PathBuf};

pub(crate) const PROGRAMS: [&str; 2] = ["north", "verify-north"];
pub(crate) const TREE: &str = "tree";
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

pub(crate) fn is_sha(name: &str) -> bool {
    name.len() == 40 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn has_programs(runtime: &Path) -> bool {
    PROGRAMS.iter().all(|program| runtime.join(program).is_file())
}

pub(crate) fn harness_root(executable: &Path) -> Option<PathBuf> {
    let promoted = executable.parent().map(|runtime| runtime.join(TREE));
    promoted
        .into_iter()
        .chain(executable.ancestors().map(Path::to_path_buf))
        .find(|root| root.join("harness/lib/usage.py").is_file())
}

pub(crate) fn checkout_for(executable: &Path, checkout: PathBuf) -> Result<PathBuf, String> {
    let Some(runtime) = promoted_sha(executable) else {
        return Ok(checkout);
    };
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(&checkout)
        .args(["rev-parse", "HEAD"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_default();
    match head == runtime {
        true => Ok(checkout),
        false => Err(format!(
            "north: runtime {runtime} has no tree and the checkout {} is at {}; run north runtime update",
            checkout.display(),
            if head.is_empty() { "no commit" } else { &head }
        )),
    }
}

fn promoted_sha(executable: &Path) -> Option<&str> {
    let runtime = executable.parent()?;
    (runtime.parent()?.file_name()? == "runtimes").then_some(())?;
    runtime.file_name()?.to_str().filter(|name| is_sha(name))
}

pub(crate) fn doomed<'a>(newest_first: &'a [String], keep: &[&str]) -> Vec<&'a str> {
    newest_first
        .iter()
        .map(String::as_str)
        .filter(|name| is_sha(name) && !keep.contains(name))
        .skip(KEPT_OLDER)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn sha(digit: char) -> String {
        digit.to_string().repeat(40)
    }

    fn usage(root: &Path) {
        fs::create_dir_all(root.join("harness/lib")).unwrap();
        fs::write(root.join("harness/lib/usage.py"), "").unwrap();
    }

    #[test]
    fn a_promoted_binary_runs_harness_code_from_its_own_tree_not_the_checkout() {
        let home = tempfile::tempdir().unwrap();
        let runtime = home.path().join(".local/state/north/runtimes").join(sha('a'));
        usage(&home.path().join("code/north/main"));
        usage(&runtime.join(TREE));
        assert_eq!(harness_root(&runtime.join("north")), Some(runtime.join(TREE)));
    }
}
