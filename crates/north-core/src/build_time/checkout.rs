use std::path::{Path, PathBuf};

pub(super) struct Checkout {
    repo: PathBuf,
    root: tempfile::TempDir,
}

impl Checkout {
    pub(super) fn detached(repo: &Path, rev: &str) -> Result<Self, String> {
        let state = super::default_ledger().parent().map(Path::to_path_buf).unwrap_or_else(std::env::temp_dir).join("build-time");
        std::fs::create_dir_all(&state).map_err(|error| format!("{}: {error}", state.display()))?;
        let root = tempfile::Builder::new().prefix("checkout-").tempdir_in(&state).map_err(|error| error.to_string())?;
        git(repo, &["fetch", "--quiet", "origin"])?;
        git(repo, &["worktree", "add", "--quiet", "--detach", &root.path().to_string_lossy(), rev])?;
        Ok(Self { repo: repo.to_path_buf(), root })
    }

    pub(super) fn path(&self) -> &Path {
        self.root.path()
    }
}

impl Drop for Checkout {
    fn drop(&mut self) {
        if let Err(error) = git(&self.repo, &["worktree", "remove", "--force", &self.root.path().to_string_lossy()]) {
            eprintln!("north code-health build-time: {error}");
        }
    }
}

fn git(repo: &Path, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new("git").arg("-C").arg(repo).args(args).stdin(std::process::Stdio::null()).output().map_err(|error| format!("git {}: {error}", args.join(" ")))?;
    if output.status.success() { Ok(()) } else { Err(format!("git {} in {}: {}", args.join(" "), repo.display(), String::from_utf8_lossy(&output.stderr).trim())) }
}
