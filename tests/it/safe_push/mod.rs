use north_core::foundation::shim;
use std::fs::File;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::support::template::Template;

mod candidate;
mod gates;
mod preflight;
mod publication;
mod scans;
mod session;

const NORTH_SHIM: &str = r#"[ -z "${SAFE_PUSH_TEST_NORTH_TRACE:-}" ] || { printf north; printf ' <%s>' "$@"; printf '\n'; } >>"$SAFE_PUSH_TEST_NORTH_TRACE"
case "$1" in
  language-gate)
    if [ -n "${SAFE_PUSH_TEST_LANGUAGE_DENY:-}" ]; then
      [ ! -s "$SAFE_PUSH_TEST_LANGUAGE_DENY" ] || { cat "$SAFE_PUSH_TEST_LANGUAGE_DENY"; exit 1; }
      exit 0
    fi
    ;;
  land-clean)
    [ -z "${SAFE_PUSH_TEST_NORTH_TRACE:-}" ] || exit 0
    ;;
  denylist)
    if [ "$2" = attest ]; then
      printf '%s\n' "$*" >>"${SAFE_PUSH_TEST_ATTEST_TRACE:-/dev/null}"
      exit "${SAFE_PUSH_TEST_ATTEST_STATUS:-0}"
    fi
    ;;
  land-actions)
    printf '%s\n' "$*" >>"${SAFE_PUSH_TEST_ACTIONS_TRACE:-/dev/null}"
    exit "${SAFE_PUSH_TEST_ACTIONS_STATUS:-0}"
    ;;
  land-red-check)
    printf 'land-red-check\n' >>"${SAFE_PUSH_TEST_RED_TRACE:-/dev/null}"
    exit "${SAFE_PUSH_TEST_RED_CHECK_STATUS:-0}"
    ;;
  capacity)
    if [ "$2" = offload ]; then
      case "$3" in
        verdict) exit 0 ;;
        free) echo 8; exit 0 ;;
        choose)
          [ -n "${SAFE_PUSH_TEST_BUILD_HOST:-}" ] || { echo 'north capacity offload: no other host has room' >&2; exit 1; }
          echo "$SAFE_PUSH_TEST_BUILD_HOST"
          exit 0
          ;;
        run)
          for last; do :; done
          printf '%s\t%s\n' "$5" "$last" >>"${SAFE_PUSH_TEST_OFFLOAD_TRACE:-/dev/null}"
          exec sh -c "$last"
          ;;
      esac
    fi
    ;;
esac
exec REAL_NORTH "$@"
"#;

static RUNS: AtomicUsize = AtomicUsize::new(0);

const GH_SHIM: &str = "[ \"$1\" = repo ] && [ -n \"$SAFE_PUSH_TEST_VISIBILITY\" ] || exit 1\necho \"$SAFE_PUSH_TEST_VISIBILITY\"\n";

const SCRUBBED: [&str; 15] = [
    "SAFE_PUSH_SELF",
    "SAFE_PUSH_WATCH",
    "SSH_CLIENT",
    "SSH2_CLIENT",
    "SSH_CONNECTION",
    "BASH_ENV",
    "ENV",
    "SAFE_PUSH_LAND_CAPACITY",
    "SAFE_PUSH_LAND_OFFLOAD_HOST",
    "SAFE_PUSH_LAND_RUNNER",
    "SAFE_PUSH_LAND_CHECK_TIMEOUT",
    "NORTH_LAND_SPANS",
    "NORTH_LAND_NORTH",
    "NORTH_NO_AUTOREAP",
    "SAFE_PUSH_OUTPUT_CLAIMED",
];

pub fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

pub fn remote_state(remote: &Path) -> String {
    git(remote, &["for-each-ref", "--sort=refname", "--format=%(refname) %(objectname)"])
}

pub fn identify(repo: &Path) {
    git(repo, &["config", "user.name", "safe-push-test"]);
    git(repo, &["config", "user.email", "safe-push-test@example.invalid"]);
}

pub fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

pub fn wait_until(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while !condition() {
        if started.elapsed() > limit {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

pub struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    pub fn new() -> Self {
        let scratch = Self { dir: tempfile::tempdir().unwrap() };
        write(&scratch.path().join("gitconfig"), "[init]\n\tdefaultBranch = main\n");
        scratch
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn state(&self) -> PathBuf {
        self.path().join("state")
    }
}

pub fn safe_push(scratch: &Scratch) -> PathBuf {
    let bin = scratch.path().join("bin");
    if bin.join("safe-push").exists() {
        return bin.join("safe-push");
    }
    std::fs::create_dir_all(&bin).unwrap();
    shim::write_script(&bin.join("north"), &NORTH_SHIM.replace("REAL_NORTH", &format!("'{}'", env!("CARGO_BIN_EXE_north"))));
    shim::write_script(&bin.join("gh"), GH_SHIM);
    symlink(env!("CARGO_BIN_EXE_north"), bin.join("safe-push")).unwrap();
    bin.join("safe-push")
}

pub struct Run<'a> {
    scratch: &'a Scratch,
    directory: PathBuf,
    arguments: Vec<String>,
    environment: Vec<(String, String)>,
}

pub struct Observed {
    pub status: Option<i32>,
    pub output: String,
}

impl Observed {
    pub fn succeeded(&self) -> bool {
        self.status == Some(0)
    }

    pub fn expect(&self, success: bool, lines: &[&str]) {
        assert_eq!(self.succeeded(), success, "{}", self.output);
        for line in lines {
            assert!(self.output.contains(line), "missing {line:?} in:\n{}", self.output);
        }
    }

    pub fn last_line(&self) -> &str {
        self.output.lines().last().unwrap_or_default()
    }
}

pub struct Running {
    child: Child,
    log: PathBuf,
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn output(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    pub fn exits_within(&mut self, limit: Duration) -> Option<ExitStatus> {
        let mut status = None;
        wait_until(limit, || {
            status = self.child.try_wait().unwrap();
            status.is_some()
        });
        status
    }

    pub fn finish(mut self) -> Observed {
        let status = self.child.wait().unwrap();
        Observed { status: status.code(), output: self.output() }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl<'a> Run<'a> {
    pub fn new(scratch: &'a Scratch, directory: &Path, arguments: &[&str]) -> Self {
        Self { scratch, directory: directory.to_owned(), arguments: arguments.iter().map(|argument| (*argument).to_owned()).collect(), environment: Vec::new() }
    }

    pub fn env(mut self, name: &str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        self.environment.push((name.to_owned(), value.as_ref().to_string_lossy().into_owned()));
        self
    }

    fn command(&self, log: &Path) -> Command {
        let scratch = self.scratch.path();
        let path = format!("{}:{}", scratch.join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let sink = File::create(log).unwrap();
        let mut command = Command::new(safe_push(self.scratch));
        command
            .args(&self.arguments)
            .current_dir(&self.directory)
            .env("PATH", path)
            .env("XDG_STATE_HOME", self.scratch.state())
            .env("GIT_CONFIG_GLOBAL", scratch.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("SAFE_PUSH_DENYLIST", scratch.join("absent-denylist.toml"))
            .env("SAFE_PUSH_SOUTH", scratch.join("absent-south"))
            .env("THREADS_DB", scratch.join("threads.db"))
            .stdin(Stdio::null())
            .stderr(Stdio::from(sink.try_clone().unwrap()))
            .stdout(Stdio::from(sink));
        for name in SCRUBBED {
            command.env_remove(name);
        }
        command.envs(self.environment.iter().map(|(name, value)| (name, value)));
        command
    }

    pub fn start(&self) -> Running {
        let log = self.scratch.path().join(format!("run-{}.log", RUNS.fetch_add(1, Ordering::Relaxed)));
        let child = self.command(&log).spawn().unwrap();
        Running { child, log }
    }

    pub fn output(&self) -> Observed {
        self.start().finish()
    }
}

pub struct Real {
    pub repo: PathBuf,
    pub remote: PathBuf,
}

static REAL: LazyLock<Template> = LazyLock::new(|| {
    Template::build(|template| {
        let (remote, repo) = (template.join("remote.git"), template.join("repo"));
        git(template, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(template, &["init", "-q", "-b", "main", "repo"]);
        git(&remote, &["config", "gc.auto", "0"]);
        git(&repo, &["config", "gc.auto", "0"]);
        git(&repo, &["config", "maintenance.auto", "false"]);
        identify(&repo);
        write(&repo.join("content.txt"), "initial\n");
        git(&repo, &["add", "content.txt"]);
        git(&repo, &["commit", "-qm", "initial"]);
        git(&repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/other"]);
        git(&repo, &["commit", "--allow-empty", "-qm", "outgoing"]);
    })
});

impl Real {
    pub fn new(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.path().join(name);
        REAL.copy_into(&root);
        let (repo, remote) = (root.join("repo"), root.join("remote.git"));
        git(&repo, &["remote", "set-url", "origin", &remote.to_string_lossy()]);
        Self { repo, remote }
    }

    pub fn remote_state(&self) -> String {
        remote_state(&self.remote)
    }

    pub fn remote_ref(&self, name: &str) -> Option<String> {
        let found = Command::new("git").current_dir(&self.remote).args(["rev-parse", "-q", "--verify", name]).output().unwrap();
        found.status.success().then(|| String::from_utf8_lossy(&found.stdout).trim().to_owned())
    }

    pub fn commit_all(&self, message: &str) {
        git(&self.repo, &["commit", "-qam", message]);
    }
}

pub struct Landing {
    pub root: PathBuf,
    pub remote: PathBuf,
    pub seed: PathBuf,
}

static LANDING: LazyLock<Template> = LazyLock::new(|| {
    Template::build(|template| {
        git(template, &["init", "--bare", "-q", "-b", "main", "remote.git"]);
        git(template, &["init", "-q", "-b", "main", "seed"]);
        let seed = template.join("seed");
        identify(&seed);
        write(&seed.join("shared.txt"), "base\n");
        git(&seed, &["add", "shared.txt"]);
        git(&seed, &["commit", "-qm", "base"]);
        git(&seed, &["push", "-q", &template.join("remote.git").to_string_lossy(), "main"]);
    })
});

impl Landing {
    pub fn new(scratch: &Scratch, name: &str) -> Self {
        let root = scratch.path().join(name);
        LANDING.copy_into(&root);
        Self { remote: root.join("remote.git"), seed: root.join("seed"), root }
    }

    pub fn main(&self) -> String {
        git(&self.remote, &["rev-parse", "main"])
    }

    pub fn lane(&self) -> PathBuf {
        let lane = self.root.join("a");
        git(&self.root, &["clone", "-q", &self.remote.to_string_lossy(), "a"]);
        identify(&lane);
        git(&lane, &["switch", "-q", "-c", "lane-a"]);
        write(&lane.join("a.txt"), "a\n");
        git(&lane, &["add", "a.txt"]);
        git(&lane, &["commit", "-qm", "change from a"]);
        lane
    }

    pub fn configure(&self, lane: &Path, settings: &[(&str, &str)], checks: &[&str], message: &str) {
        for (key, value) in settings {
            git(lane, &["config", "--file", ".safe-push", key, value]);
        }
        for check in checks {
            git(lane, &["config", "--file", ".safe-push", "--add", "land.check", &format!("a.txt :: {check}")]);
        }
        git(lane, &["add", ".safe-push"]);
        git(lane, &["commit", "-qm", message]);
    }
}

pub const CLEAN: &str = "safe-push: ✓ clean (--dry-run)";

pub struct Fixture {
    pub scratch: Scratch,
    pub root: PathBuf,
    pub repo: PathBuf,
    pub remote: PathBuf,
    pub bin: PathBuf,
    pub state: PathBuf,
    pub trace: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let scratch = Scratch::new();
        let real = Real::new(&scratch, "real");
        let bin = safe_push(&scratch).parent().unwrap().canonicalize().unwrap();
        let (repo, remote) = (real.repo.canonicalize().unwrap(), real.remote.canonicalize().unwrap());
        let root = repo.parent().unwrap().to_owned();
        std::fs::create_dir_all(root.join("home")).unwrap();
        Self { state: scratch.state(), trace: root.join("north-trace"), scratch, root, repo, remote, bin }
    }

    pub fn deny_language(&self, message: &str) {
        write(&self.root.join("language-deny"), &format!("{message}\n"));
    }

    pub fn leak_secret(&self) {
        shim::write_script(&self.bin.join("betterleaks"), "[ \"$1\" != git ] || { echo 'leaks found: 1'; exit 1; }\n");
    }

    pub fn run(&self, arguments: &[&str]) -> Observed {
        self.run_with(arguments, &[])
    }

    pub fn run_with(&self, arguments: &[&str], environment: &[(&str, &Path)]) -> Observed {
        let mut run = Run::new(&self.scratch, &self.repo, arguments)
            .env("HOME", self.root.join("home"))
            .env("SAFE_PUSH_TEST_NORTH_TRACE", &self.trace)
            .env("SAFE_PUSH_TEST_LANGUAGE_DENY", self.root.join("language-deny"));
        for (name, value) in environment {
            run = run.env(name, value);
        }
        run.output()
    }

    pub fn git(&self, arguments: &[&str]) -> String {
        git(&self.repo, arguments)
    }

    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn remote_state(&self) -> String {
        remote_state(&self.remote)
    }

    pub fn commit(&self, path: &str, content: &str, message: &str) {
        write(&self.repo.join(path), content);
        self.git(&["add", path]);
        self.git(&["commit", "-qm", message]);
    }

    pub fn land_checks(&self, checks: &[&str], message: &str) {
        let _ = Command::new("git").current_dir(&self.repo).args(["config", "--file", ".safe-push", "--unset-all", "land.check"]).status();
        for check in checks {
            self.git(&["config", "--file", ".safe-push", "--add", "land.check", check]);
        }
        self.git(&["add", ".safe-push"]);
        self.git(&["commit", "-qm", message]);
    }

    pub fn origin_named_north(&self) {
        symlink("remote.git", self.root.join("north.git")).unwrap();
        self.git(&["config", "remote.origin.pushurl", &self.remote.to_string_lossy()]);
        self.git(&["config", "remote.origin.url", &self.root.join("north.git").to_string_lossy()]);
    }

    pub fn traced(&self) -> String {
        std::fs::read_to_string(&self.trace).unwrap_or_default()
    }

    pub fn clear_trace(&self) {
        let _ = std::fs::remove_file(&self.trace);
    }
}
