use std::path::{Path, PathBuf};
use std::process::Stdio;

#[derive(Clone, Default)]
pub(super) struct Trees {
    pub(super) base: String,
    pub(super) head: String,
}

impl Trees {
    pub(super) fn of(root: &Path, base: &str, head: &str) -> Self {
        let tree = |revision: &str| {
            if revision.is_empty() {
                return String::new();
            }
            std::process::Command::new("git")
                .args(["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")])
                .current_dir(root)
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .unwrap_or_default()
        };
        Self { base: tree(base), head: tree(head) }
    }
}

pub(super) struct Checkout {
    root: PathBuf,
    back: Vec<String>,
}

fn git_in(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(arguments).current_dir(root).stdin(Stdio::null()).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Checkout {
    pub(super) fn switch(root: &Path, base: &str) -> Result<Self, String> {
        let back = match git_in(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
            Ok(branch) if !branch.is_empty() => vec![branch],
            _ => vec!["--detach".to_owned(), git_in(root, &["rev-parse", "HEAD"])?],
        };
        git_in(root, &["checkout", "-q", "--force", "--detach", base])?;
        Ok(Self { root: root.to_owned(), back })
    }

    pub(super) fn restore(self) -> Result<(), String> {
        let mut arguments = vec!["checkout", "-q", "--force"];
        arguments.extend(self.back.iter().map(String::as_str));
        git_in(&self.root, &arguments).map(drop)
    }
}
