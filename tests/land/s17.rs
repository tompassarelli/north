use north_core::foundation::shim;
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
    stdout: String,
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
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn dropped(&self, lane: &str, head: &str) -> Scan {
        self.north(&self.repo, &["dropped-commits", "--lane", lane, "--head", head, "--onto", "main"])
    }

    fn fake_bun(&self, body: &str) -> PathBuf {
        let runs = self.root.path().join("bun-runs");
        write(&runs, "");
        fs::create_dir_all(self.bin()).unwrap();
        shim::write_executable(
            &self.bin().join("bun"),
            format!("#!/bin/sh\n[ \"$*\" = \"install --frozen-lockfile\" ] || exit 2\nprintf '%s\\n' \"$PWD\" >>'{}'\n{body}\n", runs.display()),
        );
        runs
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

#[test]
fn relanded_lane_rewritten_main() {
    let fixture = Fixture::new("remote.git");
    fixture.main(&[(".safe-push", "[ci]\n\tbudget = farm-test.yml 240\n")], "main CI budget");
    git(&fixture.repo, &["pull", "-q", "--ff-only"]);
    let first = fixture.lane(&[("a.txt", "a\n")], "change from a");
    let lane = fixture.lane(&[("b.txt", "second\n")], "second change from a");
    git(&fixture.repo, &["push", "-q", "origin", "HEAD:main"]);
    git(&fixture.seed, &["pull", "-q", "--ff-only", "origin", "main"]);
    git(&fixture.seed, &["reset", "-q", "--soft", "HEAD~2"]);
    fixture.main(&[], "squash of the lane");
    let head = fixture.rebase();
    let scan = fixture.dropped(&lane, &head);
    assert_eq!(scan.status, 1, "{}", scan.stderr);
    assert!(!scan.stderr.contains("ambiguous argument"), "{}", scan.stderr);
    assert!(scan.stderr.contains(&first), "{}", scan.stderr);
}

#[test]
fn lane_already_on_main() {
    let fixture = Fixture::new("remote.git");
    fixture.lane(&[("a.txt", "a\n")], "change from a");
    let lane = fixture.lane(&[("b.txt", "second\n")], "second change from a");
    fixture.main(&[("a.txt", "a\n")], "change from a");
    fixture.main(&[("b.txt", "second\n")], "second change from a");
    fixture.main(&[("other.txt", "other\n")], "main moves on");
    let head = fixture.rebase();
    let scan = fixture.dropped(&lane, &head);
    assert_eq!(scan.status, 0, "{}", scan.stderr);
    assert!(!scan.stderr.contains("lane commits missing"), "{}", scan.stderr);
}

#[test]
fn rebase_context_only() {
    let fixture = Fixture::new("remote.git");
    let twenty: String = (1..=20).map(|line| format!("{line}\n")).collect();
    fixture.main(&[("shared.txt", &twenty)], "twenty shared lines");
    git(&fixture.repo, &["pull", "-q", "--ff-only"]);
    let edited = |lines: &[usize]| -> String {
        (1..=20)
            .map(|line| match line {
                2 if lines.contains(&2) => "two\n".to_owned(),
                18 if lines.contains(&18) => "eighteen\n".to_owned(),
                other => format!("{other}\n"),
            })
            .collect()
    };
    let lane = fixture.lane(&[("shared.txt", &edited(&[2, 18]))], "a edits two shared lines");
    fixture.main(&[("shared.txt", &edited(&[2])), ("other.txt", "other\n")], "main edits the first of them");
    let head = fixture.rebase();
    let scan = fixture.dropped(&lane, &head);
    assert_eq!(scan.status, 0, "{}", scan.stderr);
    assert!(!scan.stderr.contains("lane commits missing"), "{}", scan.stderr);
}

#[test]
fn stale_revert_before_land_checks() {
    let fixture = Fixture::new("remote.git");
    fixture.main(&[("content.txt", "moved\n")], "main moves content.txt");
    git(&fixture.repo, &["fetch", "-q", "origin"]);
    git(&fixture.repo, &["reset", "-q", "--soft", "origin/main"]);
    let head = fixture.lane(&[], "squash without a rebase");
    let base = git(&fixture.repo, &["rev-parse", "origin/main"]);
    let scan = fixture.north(&fixture.repo, &["stale-revert", "--destination", "main", "--base", &base, "--lane", &head, &head]);
    assert_eq!(scan.status, 1, "{}{}", scan.stdout, scan.stderr);
    assert!(scan.stdout.contains("safe-push: ✗ this landing undoes recent commits on main"), "{}", scan.stdout);
    let other = fixture.north(&fixture.repo, &["stale-revert", "--destination", "topic", "--base", &base, &head]);
    assert_eq!(other.status, 0, "a non-default destination is not checked: {}", other.stdout);
    let unborn = fixture.north(&fixture.repo, &["stale-revert", "--destination", "main", "--base", "", &head]);
    assert_eq!(unborn.status, 0, "a new destination is not checked: {}", unborn.stdout);
}

#[test]
fn fresh_north_worktree() {
    let fixture = Fixture::new("north.git");
    fixture.lane(
        &[
            ("agent-machinery/package.json", "{}\n"),
            ("agent-machinery/bun.lock", "{\n  \"lockfileVersion\": 1,\n  \"packages\": {}\n}\n"),
            ("agent-machinery/deep/tool/bun.lock", "pin\n"),
        ],
        "agent-machinery project",
    );
    let runs = fixture.fake_bun("mkdir -p node_modules");
    let scan = fixture.north(&fixture.repo, &["deps"]);
    let project = fixture.repo.join("agent-machinery");
    assert_eq!(scan.status, 0, "{}", scan.stderr);
    assert!(scan.stdout.contains(&format!("safe-push: no node_modules — bun install --frozen-lockfile in {}", project.display())), "{}", scan.stdout);
    assert_eq!(fs::read_to_string(&runs).unwrap(), format!("{}\n", project.display()), "only the top-level project gets a first install");
    assert!(project.join("node_modules/.safe-push-deps").is_file());
    let again = fixture.north(&fixture.repo, &["deps"]);
    assert_eq!(again.status, 0, "{}", again.stderr);
    assert_eq!(fs::read_to_string(&runs).unwrap().lines().count(), 1, "a matching stamp installs nothing: {}", again.stdout);
}

#[test]
fn sync_dependencies_from_subdirectory() {
    let fixture = Fixture::new("makgora.git");
    fixture.lane(&[("ts/package.json", "{}\n"), ("ts/bun.lock", "pin 1\n")], "ts project");
    let runs = fixture.fake_bun("mkdir -p node_modules");
    let scan = fixture.north(&fixture.repo.join("ts"), &["deps"]);
    assert_eq!(scan.status, 0, "{}", scan.stderr);
    assert_eq!(fs::read_to_string(&runs).unwrap(), format!("{}\n", fixture.repo.join("ts").display()), "{}", scan.stdout);
    assert!(!fixture.repo.join("node_modules").exists(), "installed at the repository root");
}

#[test]
fn sync_dependencies_symlinked_node_modules() {
    let fixture = Fixture::new("north.git");
    fixture.lane(&[("ts/package.json", "{}\n"), ("ts/bun.lock", "pin 1\n")], "ts project");
    let checkout = fixture.root.path().join("checkout/node_modules");
    write(&checkout.join("installed.txt"), "dependency\n");
    write(&checkout.join(".safe-push-deps"), "stale stamp\n");
    std::os::unix::fs::symlink(&checkout, fixture.repo.join("ts/node_modules")).unwrap();
    let runs = fixture.fake_bun("mkdir -p node_modules");
    let scan = fixture.north(&fixture.repo, &["deps"]);
    assert_ne!(scan.status, 0, "a symlinked node_modules whose lockfile differs was accepted: {}", scan.stdout);
    assert!(scan.stderr.contains(&format!("ts/node_modules is a symlink to {}", checkout.display())), "{}", scan.stderr);
    assert_eq!(fs::read_to_string(&runs).unwrap(), "", "bun install ran through the symlink");
    assert_eq!(fs::read_to_string(checkout.join("installed.txt")).unwrap(), "dependency\n");
    assert_eq!(fs::read_to_string(checkout.join(".safe-push-deps")).unwrap(), "stale stamp\n");
}

#[test]
fn train_deps() {
    let fixture = Fixture::new("remote.git");
    fixture.lane(&[(".safe-push", "[land]\n\tinferred = false\n"), ("package.json", "{}\n"), ("bun.lock", "pin 1\n")], "pin 1");
    fs::create_dir_all(fixture.repo.join("node_modules")).unwrap();
    write(&fixture.repo.join(".git/info/exclude"), "node_modules/\n");
    let runs = fixture.fake_bun("cp bun.lock node_modules/installed.lock");
    let installed = fixture.repo.join("node_modules/installed.lock");
    assert_eq!(fixture.north(&fixture.repo, &["deps"]).status, 0);
    assert_eq!(fs::read_to_string(&installed).unwrap(), "pin 1\n", "an unstamped install is refreshed");
    assert_eq!(fs::read_to_string(fixture.repo.join("node_modules/.safe-push-deps")).unwrap(), "d05285ffa1c79620\n", "the stamp matches the one bash wrote with sha256sum | cut -c1-16");
    fixture.lane(&[("bun.lock", "pin 2\n")], "pin 2");
    let scan = fixture.north(&fixture.repo, &["deps"]);
    assert_eq!(scan.status, 0, "{}", scan.stderr);
    assert!(scan.stdout.contains("safe-push: bun.lock changed — bun install --frozen-lockfile in"), "{}", scan.stdout);
    assert_eq!(fs::read_to_string(&installed).unwrap(), "pin 2\n", "the moved lockfile was not installed");
    assert_eq!(fs::read_to_string(&runs).unwrap().lines().count(), 2);
}
