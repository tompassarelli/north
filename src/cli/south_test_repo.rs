use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

pub fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "gex45")
        .env("GIT_AUTHOR_EMAIL", "gex45@localhost")
        .env("GIT_COMMITTER_NAME", "gex45")
        .env("GIT_COMMITTER_EMAIL", "gex45@localhost")
        .args(["-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", "-c", "protocol.file.allow=always"])
        .args(arguments)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub struct Repo {
    pub directory: TempDir,
}

impl Repo {
    pub fn new() -> Self {
        let directory = TempDir::new().expect("tempdir");
        git(directory.path(), &["init", "-q"]);
        Self { directory }
    }

    pub fn path(&self) -> &Path {
        self.directory.path()
    }

    pub fn write(&self, relative: &str, content: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, content).expect("write");
    }

    pub fn symlink(&self, relative: &str, target: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::os::unix::fs::symlink(target, path).expect("symlink");
    }

    pub fn commit_all(&self, message: &str) -> String {
        git(self.path(), &["add", "-A"]);
        git(self.path(), &["commit", "-q", "-m", message]);
        self.head()
    }

    pub fn head(&self) -> String {
        git(self.path(), &["rev-parse", "HEAD"])
    }
}

pub fn raw_diff(repo: &Repo, base: &str) -> Vec<u8> {
    let range = format!("{base}..HEAD");
    Command::new("git")
        .current_dir(repo.path())
        .args(["diff", "--raw", "--no-renames", "-z", &range])
        .output()
        .expect("git diff")
        .stdout
}
