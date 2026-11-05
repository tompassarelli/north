use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GITLEAKS: &str = r#"#!/bin/sh
printf '%s\n' "$*" >>"$FIXTURE/gitleaks.trace"
if [ -n "${RACE:-}" ] && [ ! -e "$FIXTURE/raced" ]; then
  case "$RACE" in
    branch) git -C "$FIXTURE/repo" switch -q -c raced-branch ;;
    commit) git -C "$FIXTURE/repo" commit --allow-empty -qm 'raced commit' ;;
    destination) git --git-dir="$FIXTURE/remote.git" update-ref refs/heads/other "$DESTINATION_OID" ;;
    worktree)
      mv "$FIXTURE/repo/.git" "$FIXTURE/repo/.git.original"
      printf 'gitdir: %s\n' "$FIXTURE/other/.git" >"$FIXTURE/repo/.git"
      ;;
  esac
  : >"$FIXTURE/raced"
fi
exit 0
"#;

const LEASE_GIT: &str = r#"#!/bin/sh
if [ "$1" = push ]; then
  "$REAL_GIT" --git-dir="$FIXTURE/remote.git" update-ref refs/heads/other "$LEASE_OID"
  : >"$FIXTURE/raced"
fi
exec "$REAL_GIT" "$@"
"#;

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    root: tempfile::TempDir,
    destination_oid: String,
    base_oid: String,
}

struct Run {
    status: i32,
    output: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let top = root.path().to_path_buf();
        let top = top.as_path();
        fs::write(top.join("gitconfig"), "[init]\n\tdefaultBranch = main\n[user]\n\tname = safe-push-test\n\temail = safe-push-test@example.invalid\n").unwrap();
        let fixture = Self { root, destination_oid: String::new(), base_oid: String::new() };
        let repo = fixture.repo();
        fixture.git_at(top, &["init", "-q", "--bare", "remote.git"]);
        fixture.git_at(top, &["init", "-q", "-b", "main", "repo"]);
        fs::write(repo.join("content.txt"), "initial\n").unwrap();
        fixture.git(&["add", "content.txt"]);
        fixture.git(&["commit", "-qm", "initial"]);
        let base_oid = fixture.git(&["rev-parse", "HEAD"]);
        fixture.git(&["remote", "add", "origin", &top.join("remote.git").to_string_lossy()]);
        fixture.git(&["push", "-q", "-u", "origin", "main"]);
        fixture.git(&["push", "-q", "origin", "HEAD:refs/heads/other"]);
        fixture.git(&["commit", "--allow-empty", "-qm", "outgoing"]);
        fixture.git_at(top, &["init", "-q", "-b", "main", "other"]);
        let other = top.join("other");
        fixture.git_at(&other, &["commit", "--allow-empty", "-qm", "other"]);
        fixture.git_at(&other, &["remote", "add", "origin", &top.join("remote.git").to_string_lossy()]);
        fixture.git_at(&other, &["push", "-q", "origin", "HEAD:refs/heads/race-target"]);
        let destination_oid = fixture.git_at(&other, &["rev-parse", "HEAD"]);
        let bin = top.join("bin");
        fs::create_dir_all(&bin).unwrap();
        shim::write_executable(&bin.join("gitleaks"), GITLEAKS);
        shim::write_executable(&bin.join("gh"), "#!/bin/sh\necho PUBLIC\n");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), bin.join("north")).unwrap();
        Self { destination_oid, base_oid, ..fixture }
    }

    fn top(&self) -> &Path {
        self.root.path()
    }

    fn repo(&self) -> PathBuf {
        self.top().join("repo")
    }

    fn git_at(&self, directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", self.top().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn git(&self, arguments: &[&str]) -> String {
        self.git_at(&self.repo(), arguments)
    }

    fn remote(&self) -> String {
        git(&self.top().join("remote.git"), &["for-each-ref", "--format=%(refname) %(objectname)"])
    }

    fn remote_ref(&self, name: &str) -> Option<String> {
        let output = Command::new("git").arg("--git-dir").arg(self.top().join("remote.git")).args(["rev-parse", "-q", "--verify", name]).output().unwrap();
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    fn scanned(&self) -> String {
        fs::read_to_string(self.top().join("gitleaks.trace")).unwrap_or_default()
    }

    fn upstream(&self) -> bool {
        Command::new("git").current_dir(self.repo()).args(["rev-parse", "--verify", "@{upstream}"]).stderr(Stdio::null()).status().unwrap().success()
    }

    fn command(&self, program: &[&str], environment: &[(&str, &str)]) -> Command {
        let path = format!("{}:{}", self.top().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(program[0]);
        command
            .args(&program[1..])
            .current_dir(self.repo())
            .env("PATH", path)
            .env("HOME", self.top().join("home"))
            .env("XDG_STATE_HOME", self.top().join("state"))
            .env("GIT_CONFIG_GLOBAL", self.top().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("SAFE_PUSH_DENYLIST", self.top().join("absent-denylist.toml"))
            .env("SAFE_PUSH_SOUTH", self.top().join("absent-south"))
            .env("FIXTURE", self.top())
            .env("DESTINATION_OID", &self.destination_oid)
            .env_remove("SAFE_PUSH_OUTPUT_CLAIMED")
            .env_remove("SAFE_PUSH_LOCK_HELD")
            .env_remove("NORTH_NO_AUTOREAP")
            .env_remove("NORTH_FLEET_SESSION")
            .envs(environment.iter().copied())
            .stdin(Stdio::null());
        command
    }

    fn push(&self, arguments: &[&str], environment: &[(&str, &str)]) -> Run {
        let mut program = vec![env!("CARGO_BIN_EXE_north"), "land", "push"];
        program.extend(arguments);
        let output = self.command(&program, environment).output().unwrap();
        let worktree = self.repo().join(".git.original");
        if worktree.exists() {
            fs::remove_file(self.repo().join(".git")).unwrap();
            fs::rename(worktree, self.repo().join(".git")).unwrap();
        }
        Run { status: output.status.code().unwrap(), output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }
    }
}

#[test]
fn argument_boundary() {
    let fixture = Fixture::new();
    shim::write_executable(&fixture.top().join("bin/git"), "#!/bin/sh\necho called >>\"$FIXTURE/gitleaks.trace\"\nexit 97\n");
    let before = fixture.remote();
    for row in ["--to main --tag release-a", "--dry-run --dry-run", "--keep-lane", "--to main --to other", "--tag release-a --tag release-b", "--help --dry-run", "--to bad..name"] {
        let run = fixture.push(&row.split(' ').collect::<Vec<_>>(), &[]);
        assert_ne!(run.status, 0, "{row}: {}", run.output);
        assert_eq!(fixture.scanned(), "", "{row} reached git or a scan");
        assert_eq!(fixture.remote(), before, "{row} changed origin");
    }
}

#[test]
fn planning_outcomes() {
    let fixture = Fixture::new();
    let before = fixture.remote();
    let run = fixture.push(&["--dry-run", "--to", "other"], &[]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote(), before, "dry-run changed origin");
    let main = fixture.remote_ref("main");
    let run = fixture.push(&["--to", "other"], &[]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("other"), Some(fixture.git(&["rev-parse", "HEAD"])), "explicit destination did not receive HEAD");
    assert_eq!(fixture.remote_ref("main"), main, "explicit destination changed main");
    let run = fixture.push(&["--dry-run"], &[]);
    assert_eq!(run.status, 2, "a bare push of the default branch must land through safe-push --to main: {}", run.output);
}

#[test]
fn normal() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "topic"]);
    let run = fixture.push(&["--to", "topic"], &[]);
    assert_eq!(run.status, 0, "{}", run.output);
    let head = fixture.git(&["rev-parse", "HEAD"]);
    assert_eq!(fixture.remote_ref("topic"), Some(head.clone()), "publication did not update origin to the captured HEAD");
    assert!(run.output.contains(&format!("safe-push: pushed {head} to topic")), "{}", run.output);
}

#[test]
fn lease_boundary() {
    let fixture = Fixture::new();
    let lease_oid = fixture.git(&["rev-parse", "HEAD"]);
    fixture.git(&["push", "-q", "origin", &format!("{lease_oid}:refs/heads/lease-peer")]);
    fixture.git(&["commit", "--allow-empty", "-qm", "after intervening tip"]);
    shim::write_executable(&fixture.top().join("bin/git"), LEASE_GIT);
    let real_git = String::from_utf8(Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let run = fixture.push(&["--to", "other"], &[("REAL_GIT", real_git.trim()), ("LEASE_OID", &lease_oid)]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(fixture.top().join("raced").exists(), "the lease race was not injected");
    assert_eq!(fixture.remote_ref("other"), Some(lease_oid), "publication overwrote a destination that moved after revalidation");
}

fn race(name: &str) {
    let fixture = Fixture::new();
    let before = fixture.remote();
    let run = fixture.push(&["--to", "other"], &[("RACE", name)]);
    assert_ne!(run.status, 0, "{name}: {}", run.output);
    assert!(fixture.top().join("raced").exists(), "the {name} race was not injected");
    if name == "destination" {
        assert_eq!(fixture.remote_ref("other").as_deref(), Some(fixture.destination_oid.as_str()), "the destination race was not the sole remote mutation");
        assert!(!run.output.contains("pushing main -> origin"), "published after the destination changed: {}", run.output);
    } else {
        assert_eq!(fixture.remote(), before, "the {name} race mutated origin despite the fail-closed verdict");
    }
    if name != "worktree" {
        assert!(run.output.contains("repository state changed during secret scan"), "{name}: {}", run.output);
    }
}

#[test]
fn race_branch() {
    race("branch");
}

#[test]
fn race_commit() {
    race("commit");
}

#[test]
fn race_destination() {
    race("destination");
}

#[test]
fn race_worktree() {
    race("worktree");
}

#[test]
fn no_upstream() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "topic"]);
    fixture.git(&["commit", "--allow-empty", "-qm", "topic"]);
    let before = fixture.remote();
    let run = fixture.push(&[], &[]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("one-branch-main doctrine"), "{}", run.output);
    assert_eq!(fixture.remote(), before, "the non-main refusal mutated origin");
    assert_eq!(fixture.scanned(), "", "the non-main refusal reached a scan");
    assert!(!fixture.upstream(), "the non-main refusal established tracking");
}

#[test]
fn inherited_upstream() {
    let fixture = Fixture::new();
    let main = fixture.remote_ref("main");
    fixture.git(&["switch", "-q", "-c", "feature"]);
    fixture.git(&["commit", "--allow-empty", "-qm", "feature"]);
    fixture.git(&["branch", "--set-upstream-to=origin/main", "feature"]);
    let run = fixture.push(&[], &[]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("main"), main, "an inherited origin/main upstream let the refused push touch main");
    assert_eq!(fixture.remote_ref("feature"), None, "the non-main refusal created a remote feature branch");
}

#[test]
fn dry_run_destination() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "feature"]);
    fixture.git(&["commit", "--allow-empty", "-qm", "feature"]);
    let before = fixture.remote();
    let run = fixture.push(&["--dry-run"], &[]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote(), before, "dry-run mutated origin");
    assert!(!fixture.upstream(), "dry-run established tracking");
}

#[test]
fn new_destination() {
    let fixture = Fixture::new();
    let main = fixture.remote_ref("main").unwrap();
    fixture.git(&["switch", "-q", "-c", "scratch"]);
    fixture.git(&["commit", "--allow-empty", "-qm", "scratch"]);
    let head = fixture.git(&["rev-parse", "HEAD"]);
    let run = fixture.push(&["--to", "farm/scan-test"], &[]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("farm/scan-test"), Some(head.clone()), "the new destination was not created at HEAD");
    assert!(fixture.scanned().contains(&format!("--log-opts={main}..{head}")), "a new branch scanned more than origin's default branch lacks: {}", fixture.scanned());
}

#[test]
fn non_fast_forward() {
    let fixture = Fixture::new();
    fixture.git(&["fetch", "-q", "origin", "race-target:refs/remotes/origin/race-target"]);
    fixture.git(&["switch", "-q", "-c", "feature", &fixture.base_oid]);
    fixture.git(&["commit", "--allow-empty", "-qm", "feature"]);
    let before = fixture.remote();
    let run = fixture.push(&["--to", "race-target"], &[]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("non-fast-forward"), "{}", run.output);
    assert_eq!(fixture.remote(), before, "the non-fast-forward refusal mutated origin");
    assert_eq!(fixture.scanned(), "", "the non-fast-forward refusal reached gitleaks");
}

#[test]
fn symbolic_destination() {
    let fixture = Fixture::new();
    git(&fixture.top().join("remote.git"), &["symbolic-ref", "refs/heads/alias", "refs/heads/main"]);
    let before = fixture.remote();
    let run = fixture.push(&["--to", "alias"], &[]);
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("is symbolic"), "{}", run.output);
    assert_eq!(fixture.remote(), before, "the symbolic destination refusal mutated origin");
}

#[test]
fn farm_scratch() {
    let fixture = Fixture::new();
    fixture.git(&["switch", "-q", "-c", "lane"]);
    let origin = fixture.git(&["remote", "get-url", "--push", "origin"]);
    let key = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(origin.as_bytes()));
    let locks = fixture.top().join("state/safe-push");
    fs::create_dir_all(&locks).unwrap();
    let lock = locks.join(format!("{}.lock", &key[..16]));
    let mut holder = Command::new("flock").arg(&lock).args(["sleep", "60"]).spawn().unwrap();
    while Command::new("flock").arg("-n").arg(&lock).arg("true").status().unwrap().success() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let safe_push = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
    let output = fixture.command(&["timeout", "60", safe_push.to_str().unwrap(), "--to", "farm/x"], &[]).output().unwrap();
    holder.kill().unwrap();
    holder.wait().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "{text}");
    assert!(!text.contains("waiting for the landing lock"), "the farm scratch push waited for the landing lock: {text}");
    assert_eq!(fixture.remote_ref("farm/x"), Some(fixture.git(&["rev-parse", "HEAD"])), "the farm scratch push did not publish farm/x");
}

impl Fixture {
    fn restore(&self, arguments: &[&str]) -> Run {
        let mirrors = self.top().join("mirrors");
        let mut program = vec![env!("CARGO_BIN_EXE_north"), "land", "restore"];
        program.extend(arguments);
        let output = self.command(&program, &[("LANE_GC_MIRRORS", &mirrors.to_string_lossy())]).output().unwrap();
        Run { status: output.status.code().unwrap(), output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }
    }

    fn deleted_branch(&self, name: &str) -> String {
        self.git(&["switch", "-q", "-c", name]);
        fs::write(self.repo().join("work.txt"), name).unwrap();
        self.git(&["add", "work.txt"]);
        self.git(&["commit", "-qm", name]);
        let oid = self.git(&["rev-parse", "HEAD"]);
        self.git(&["push", "-q", "origin", &format!("HEAD:refs/heads/{name}")]);
        let mirror = self.top().join("mirrors/remote.git");
        fs::create_dir_all(&mirror).unwrap();
        self.git_at(&mirror, &["init", "-q", "--bare", "."]);
        self.git_at(&mirror, &["fetch", "-q", &self.top().join("remote.git").to_string_lossy(), &format!("refs/heads/{name}:refs/archive/2026-11-05/{name}")]);
        self.git(&["push", "-q", "origin", &format!(":refs/heads/{name}")]);
        self.git(&["switch", "-q", "main"]);
        self.git(&["branch", "-qD", name]);
        oid
    }
}

#[test]
fn restore_republishes_a_mirror_held_commit_without_scans() {
    let fixture = Fixture::new();
    let oid = fixture.deleted_branch("claude/lost");
    assert_eq!(fixture.remote_ref("claude/lost"), None);
    let run = fixture.restore(&["--dry-run", &oid, "claude/lost"]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("claude/lost"), None, "dry-run restored the branch");
    let run = fixture.restore(&[&oid, "claude/lost"]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("claude/lost"), Some(oid.clone()), "{}", run.output);
    assert_eq!(fixture.scanned(), "", "a restore ran a content scan");
    let run = fixture.restore(&[&oid, "claude/lost"]);
    assert_eq!(run.status, 1, "restore moved or re-pushed an existing branch: {}", run.output);
}

#[test]
fn restore_republishes_a_commit_reachable_from_the_default_branch() {
    let fixture = Fixture::new();
    let base = fixture.base_oid.clone();
    let run = fixture.restore(&[&base, "again"]);
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.remote_ref("again"), Some(base));
}

#[test]
fn restore_refuses_what_is_not_public_or_would_move_a_branch() {
    let fixture = Fixture::new();
    let private = fixture.git(&["rev-parse", "HEAD"]);
    let before = fixture.remote();
    for row in [
        vec![private.as_str(), "lost"],
        vec![fixture.base_oid.as_str(), "main"],
        vec![fixture.base_oid.as_str(), "other"],
        vec![fixture.base_oid.as_str(), "bad..name"],
        vec!["abc123", "lost"],
        vec![fixture.base_oid.as_str()],
    ] {
        let run = fixture.restore(&row);
        assert_ne!(run.status, 0, "{row:?}: {}", run.output);
        assert_eq!(fixture.remote(), before, "{row:?} changed origin");
    }
}
