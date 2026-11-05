use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const NORTH_STUB: &str = r#"printf 'north %s\n' "$*" >>"$STUB_TRACE"
case "$1" in
  land-cache) exit 1 ;;
  land-red-check) exit 0 ;;
  capacity)
    case "$2 $3" in
      "offload choose") printf 'buildbox\n'; exit 0 ;;
      "offload free") printf '%s\n' "$STUB_FREE_SLOTS"; exit 0 ;;
      "offload run") case "$*" in *"-- ${STUB_BUSY_COMMAND:-}") exit 96 ;; esac; exit 0 ;;
    esac
    ;;
esac
exec "$REAL_NORTH" "$@"
"#;

const FAKE_GH: &str = r#"printf 'gh %s\n' "$*" >>"$STUB_TRACE"
case "$1 $2" in
  "workflow run")
    while [ $# -gt 0 ]; do
      case "$1" in
        -f) case "$2" in head=*) head=${2#head=} ;; nonce=*) nonce=${2#nonce=} ;; esac; shift ;;
      esac
      shift
    done
    printf 'Land check %s %s' "$head" "$nonce" >"$GH_STATE/title"
    ;;
  "run list") printf '[{"databaseId":7,"displayTitle":"%s","url":"https://github.invalid/runs/7","status":"completed","conclusion":"success"}]\n' "$(cat "$GH_STATE/title")" ;;
  *) exit 1 ;;
esac
"#;

const PORTABLE: &str = "portable-check";
const BOUND: &str = "bound-check";

struct Fixture {
    root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
    head: String,
}

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

fn script(path: &Path, body: &str) {
    shim::write_executable(path, format!("#!/usr/bin/env bash\nset -u\n{body}"));
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        git(root.path(), &["init", "-q", "--bare", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", root.path().join("origin.git").to_str().unwrap()]);
        fs::write(repo.join("file.txt"), "base\n").unwrap();
        git(&repo, &["add", "file.txt"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        fs::write(repo.join("file.txt"), "changed\n").unwrap();
        for (key, value) in [
            ("land.workflow", "land-check.yml"),
            ("land.runner", "build-host"),
            ("land.check", &format!("file.txt :: {PORTABLE}")),
            ("land.check", &format!("file.txt :: {BOUND}")),
            ("land.machine-bound", &format!("reads the checkouts :: {BOUND}")),
        ] {
            git(&repo, &["config", "--file", ".safe-push", "--add", key, value]);
        }
        git(&repo, &["add", "file.txt", ".safe-push"]);
        git(&repo, &["commit", "-qm", "head"]);
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let stubs = root.path().join("stubs");
        for directory in [&stubs, &root.path().join("gh"), &root.path().join("state"), &root.path().join("runtime")] {
            fs::create_dir_all(directory).unwrap();
        }
        script(&stubs.join("north"), NORTH_STUB);
        script(&stubs.join("gh"), FAKE_GH);
        script(&stubs.join("safe-push"), "exit 0\n");
        Self { root, repo, base, head }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn check(&self, free_slots: &str, busy_command: &str) -> Output {
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", &self.base, "--head", &self.head, "--safe-push"])
            .arg(self.path("stubs/safe-push"))
            .arg("--outcome")
            .arg(self.path("outcome"))
            .env("PATH", path)
            .env("NORTH_LAND_NORTH", self.path("stubs/north"))
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("STUB_TRACE", self.path("trace"))
            .env("STUB_FREE_SLOTS", free_slots)
            .env("STUB_BUSY_COMMAND", busy_command)
            .env("GH_STATE", self.path("gh"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env_remove("AGENT_CAPACITY_LEASE")
            .env_remove("SAFE_PUSH_LAND_CAPACITY")
            .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
            .env_remove("SAFE_PUSH_LAND_RUNNER")
            .env_remove("SAFE_PUSH_LAND_CHECK_TIMEOUT")
            .env_remove("GITHUB_ACTIONS")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.path("trace")).unwrap_or_default()
    }
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn offloaded(trace: &str, command: &str) -> bool {
    trace.lines().any(|line| line.starts_with("north capacity offload run --host buildbox ") && line.ends_with(&format!("-- {command}")))
}

fn dispatched(trace: &str) -> usize {
    trace.lines().filter(|line| line.starts_with("gh workflow run land-check.yml ")).count()
}

fn recorded(trace: &str, command: &str, class: &str) -> bool {
    trace.lines().any(|line| line.starts_with("north land-cache record ") && line.contains(&format!("--command {command} --host-class {class} --status 0")))
}

#[test]
fn a_free_warm_slot_runs_the_candidate_and_actions_gets_no_dispatch() {
    let fixture = Fixture::new();
    let output = fixture.check("1", "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    assert!(trace.contains("north capacity offload free --host buildbox"), "{trace}");
    assert!(offloaded(&trace, PORTABLE) && offloaded(&trace, BOUND), "{trace}");
    assert_eq!(dispatched(&trace), 0, "{trace}");
    assert!(recorded(&trace, PORTABLE, "buildbox"), "{trace}");
}

#[test]
fn every_slot_busy_checks_the_portable_part_on_actions_and_records_its_verdict() {
    let fixture = Fixture::new();
    let output = fixture.check("0", "");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    assert_eq!(dispatched(&trace), 1, "{trace}");
    assert!(trace.lines().any(|line| line.starts_with("north land-actions --workflow land-check.yml ") && line.ends_with(&format!("-- {PORTABLE}"))), "{trace}");
    assert!(!offloaded(&trace, PORTABLE), "{trace}");
    assert!(offloaded(&trace, BOUND), "a machine-bound check left the build host:\n{trace}");
    assert!(recorded(&trace, PORTABLE, "actions"), "{trace}");
    assert!(text(&output).contains("no build-host slot is free, so 1 land check(s) overflow to GitHub Actions"), "{}", text(&output));
}

#[test]
fn a_slot_wait_that_runs_out_mid_run_overflows_to_actions_instead_of_failing() {
    let fixture = Fixture::new();
    let output = fixture.check("1", PORTABLE);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    assert_eq!(dispatched(&trace), 1, "{trace}");
    assert!(recorded(&trace, PORTABLE, "actions"), "{trace}");
    assert!(text(&output).contains(&format!("so land check {PORTABLE} overflows to GitHub Actions")), "{}", text(&output));
    assert!(offloaded(&trace, BOUND), "{trace}");
}
