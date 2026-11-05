use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BETTERLEAKS: &str = r#"#!/bin/sh
printf '%s\n' "$*" >>"$FIXTURE/betterleaks.trace"
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
        shim::write_executable(&bin.join("betterleaks"), BETTERLEAKS);
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
        fs::read_to_string(self.top().join("betterleaks.trace")).unwrap_or_default()
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
fn race_commit() {
    race("commit");
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
    assert_eq!(fixture.scanned(), "", "the non-fast-forward refusal reached betterleaks");
}

impl Fixture {
    fn restore(&self, arguments: &[&str]) -> Run {
        let mirrors = self.top().join("mirrors");
        let mut program = vec![env!("CARGO_BIN_EXE_north"), "land", "restore"];
        program.extend(arguments);
        let output = self.command(&program, &[("LANE_GC_MIRRORS", &mirrors.to_string_lossy())]).output().unwrap();
        Run { status: output.status.code().unwrap(), output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }
    }
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
