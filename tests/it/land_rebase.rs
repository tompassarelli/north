use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    seed: PathBuf,
}

struct Scan {
    status: i32,
    stderr: String,
}

impl Fixture {
    fn new(origin: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join(origin);
        git(root.path(), &["init", "-q", "--bare", "-b", "main", remote.to_str().unwrap()]);
        let seed = root.path().join("seed");
        git(root.path(), &["init", "-q", "-b", "main", seed.to_str().unwrap()]);
        git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        write(&seed.join("content.txt"), "initial\n");
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-qm", "initial"]);
        git(&seed, &["push", "-q", "origin", "main"]);
        let repo = root.path().join("repo");
        git(root.path(), &["clone", "-q", remote.to_str().unwrap(), repo.to_str().unwrap()]);
        Self { root, repo, seed }
    }

    fn commit(directory: &Path, files: &[(&str, &str)], message: &str) -> String {
        for (path, text) in files {
            write(&directory.join(path), text);
        }
        git(directory, &["add", "-A"]);
        git(directory, &["commit", "-qm", message]);
        git(directory, &["rev-parse", "HEAD"])
    }

    fn lane(&self, files: &[(&str, &str)], message: &str) -> String {
        Self::commit(&self.repo, files, message)
    }

    fn main(&self, files: &[(&str, &str)], message: &str) {
        Self::commit(&self.seed, files, message);
        git(&self.seed, &["push", "-q", "-f", "origin", "main"]);
    }

    fn rebase(&self) -> String {
        git(&self.repo, &["fetch", "-q", "origin"]);
        git(&self.repo, &["rebase", "-q", "origin/main"]);
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn bin(&self) -> PathBuf {
        self.root.path().join("bin")
    }

    fn north(&self, directory: &Path, arguments: &[&str]) -> Scan {
        let path = format!("{}:{}", self.bin().display(), std::env::var("PATH").unwrap_or_default());
        let output: Output = Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(directory)
            .args(["land", "scan"])
            .args(arguments)
            .env("HOME", self.root.path().join("home"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("PATH", path)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Scan {
            status: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn dropped(&self, lane: &str, head: &str) -> Scan {
        self.north(&self.repo, &["dropped-commits", "--lane", lane, "--head", head, "--onto", "main"])
    }
}

#[test]
fn rebase_drops_commit() {
    let fixture = Fixture::new("remote.git");
    let dropped = fixture.lane(&[("a.txt", "a\n")], "change from a");
    let lane = fixture.lane(&[("b.txt", "second\n")], "second change from a");
    fixture.main(&[("a.txt", "a\n"), ("other.txt", "other\n")], "a.txt and more from elsewhere");
    let head = fixture.rebase();
    let scan = fixture.dropped(&lane, &head);
    assert_eq!(scan.status, 1, "{}", scan.stderr);
    assert!(scan.stderr.contains(&format!("lane commits missing from {head} after the rebase onto origin main: {dropped}")), "{}", scan.stderr);
    assert!(scan.stderr.trim_end().ends_with("; NOT pushed."), "{}", scan.stderr);
}

