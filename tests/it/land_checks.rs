use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const NORTH_STUB: &str = r#"printf 'north %s\n' "$*" >>"$STUB_TRACE"
case "$1" in
  land-cache) exit 1 ;;
  capacity)
    case "$2 $3" in
      "offload choose") printf 'buildbox\n'; exit 0 ;;
      "offload run") exit 0 ;;
    esac
    ;;
  land-red-check) exit "${STUB_RED_STATUS:-0}" ;;
  land-actions)
    if [ -n "${STUB_ACTIONS_RELEASE:-}" ]; then
      touch "$STUB_ACTIONS_RELEASE.waiting"
      read -r _ <"$STUB_ACTIONS_RELEASE"
    fi
    exit "${STUB_ACTIONS_STATUS:-0}"
    ;;
esac
exec "$REAL_NORTH" "$@"
"#;

const SCAN_STUB: &str = "printf '%s\\n' \"$*\" >>\"$STUB_TRACE\"\nexit \"${STUB_SCAN_STATUS:-0}\"\n";

const LEASE_PROBE: &str = "printf 'acquire\\n' >>\"$LEASE_LOG\"\ntouch \"$LEASE_HELD\"\nstatus=0\n\"$@\" || status=$?\nrm -f \"$LEASE_HELD\"\nprintf 'release\\n' >>\"$LEASE_LOG\"\nexit \"$status\"\n";

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
    fn new(settings: &[(&str, &str)]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let origin = root.path().join("origin.git");
        fs::create_dir_all(&repo).unwrap();
        git(root.path(), &["init", "-q", "--bare", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        fs::write(repo.join("file.txt"), "base\n").unwrap();
        git(&repo, &["add", "file.txt"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        fs::write(repo.join("file.txt"), "changed\n").unwrap();
        for (key, value) in settings {
            git(&repo, &["config", "--file", ".safe-push", "--add", key, value]);
        }
        git(&repo, &["add", "file.txt", ".safe-push"]);
        git(&repo, &["commit", "-qm", "head"]);
        let head = git(&repo, &["rev-parse", "HEAD"]);
        let stubs = root.path().join("stubs");
        fs::create_dir_all(&stubs).unwrap();
        script(&stubs.join("north"), NORTH_STUB);
        script(&stubs.join("safe-push"), SCAN_STUB);
        script(&stubs.join("lease-probe.sh"), LEASE_PROBE);
        fs::create_dir_all(root.path().join("state")).unwrap();
        fs::create_dir_all(root.path().join("runtime")).unwrap();
        Self { root, repo, base, head }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        command
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", &self.base, "--head", &self.head, "--safe-push"])
            .arg(self.path("stubs/safe-push"))
            .arg("--outcome")
            .arg(self.path("outcome"))
            .env("PATH", path)
            .env("NORTH_LAND_NORTH", self.path("stubs/north"))
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("STUB_TRACE", self.path("trace"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("runtime"))
            .env("LEASE_LOG", self.path("lease-log"))
            .env("LEASE_HELD", self.path("held"))
            .env_remove("AGENT_CAPACITY_LEASE")
            .env_remove("SAFE_PUSH_LAND_CAPACITY")
            .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
            .env_remove("SAFE_PUSH_LAND_RUNNER")
            .env_remove("SAFE_PUSH_LAND_CHECK_TIMEOUT")
            .stdin(Stdio::null());
        command
    }

    fn with_lease(&self) -> Command {
        let mut command = self.command();
        command.env("SAFE_PUSH_LAND_CAPACITY", format!("bash {}", self.path("stubs/lease-probe.sh").display()));
        command
    }

    fn trace(&self) -> String {
        fs::read_to_string(self.path("trace")).unwrap_or_default()
    }

    fn logs(&self) -> Vec<PathBuf> {
        fs::read_dir(self.path("state/safe-push"))
            .map(|entries| entries.flatten().map(|entry| entry.path()).filter(|path| path.to_string_lossy().contains("land-check-")).collect())
            .unwrap_or_default()
    }

    fn lease_events(&self) -> Vec<String> {
        fs::read_to_string(self.path("lease-log")).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < Duration::from_secs(30), "{} never appeared", path.display());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn finishes_within(mut child: Child, seconds: u64) -> (Option<i32>, Duration) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(seconds) {
        if let Some(status) = child.try_wait().unwrap() {
            return (status.code(), start.elapsed());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    (None, start.elapsed())
}

#[test]
fn land_check_boundary() {
    let failing = r#"printf "ran\n" >> check-runs; printf "PASS before\n"; printf "FAIL named-land-case\n" >&2; printf "PASS after\n"; exit 23"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt,other.txt :: {failing}")), ("land.check", "unmatched/ :: exit 71")]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(23), "{}", text(&output));
    let logs = fixture.logs();
    assert_eq!(logs.len(), 1, "{logs:?}");
    let expected = format!("safe-push: land check {failing} exited 23; log: {}; NOT pushed.", logs[0].display());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&expected), "{}", text(&output));
    assert!(logs[0].starts_with(fs::canonicalize(fixture.path("state/safe-push")).unwrap()));
    let log = fs::read_to_string(&logs[0]).unwrap();
    assert_eq!(log.len(), "PASS before\nFAIL named-land-case\nPASS after\n".len());
    for line in ["PASS before", "FAIL named-land-case", "PASS after"] {
        assert!(log.lines().any(|found| found == line), "{log}");
    }
    assert_eq!(fs::read_to_string(fixture.repo.join("check-runs")).unwrap(), "ran\n");
    assert!(fs::read_to_string(fixture.path("outcome")).unwrap().contains("event\tland check\tfailed\t"));

    let passing = Fixture::new(&[("land.check", "file.txt :: printf ok")]);
    let output = passing.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(passing.logs().is_empty(), "a passing land check kept its log");
    assert!(!passing.trace().contains("land-clean"));
}

#[test]
fn land_publish() {
    let probe = r#"printf '%s\n' "$SAFE_PUSH_LAND_REF" > ../published-ref; git --git-dir=../origin.git rev-parse "refs/heads/$SAFE_PUSH_LAND_REF" > ../published-head"#;
    let fixture = Fixture::new(&[("land.publish", "farm/"), ("land.check", &format!("file.txt :: {probe}"))]);
    let output = fixture.command().env("STUB_SCAN_STATUS", "0").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let reference = format!("farm/{}", &fixture.head[..12]);
    assert_eq!(fs::read_to_string(fixture.path("published-ref")).unwrap().trim(), reference);
    assert_eq!(fs::read_to_string(fixture.path("published-head")).unwrap().trim(), fixture.head);
    let remaining = Command::new("git").args(["--git-dir"]).arg(fixture.path("origin.git")).args(["rev-parse", "-q", "--verify", &format!("refs/heads/{reference}")]).output().unwrap();
    assert!(!remaining.status.success(), "the published ref outlived the land checks");
    assert!(fixture.trace().contains(&format!("--publication-scan {} {}", fixture.base, fixture.head)));
    assert!(text(&output).contains(&format!("published {} to origin {reference} for the land checks", &fixture.head[..12])));

    let refused = Fixture::new(&[("land.publish", "farm/"), ("land.check", "file.txt :: touch ../ran")]);
    let output = refused.command().env("STUB_SCAN_STATUS", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!refused.path("ran").exists(), "a land check ran after the publication scan refused");
    assert!(fs::read_to_string(refused.path("outcome")).unwrap().contains("skip\tskipped: the lane cannot be published for them until the publication scan passes"));

    let dry = Fixture::new(&[("land.publish", "farm/"), ("land.check", "file.txt :: touch ../ran")]);
    let output = dry.command().args(["--dry", "1"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(!dry.trace().contains("--publication-scan"), "a dry run published");
}

#[test]
fn dead_check_orphans() {
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: sleep 40 & echo $! >'{}'; exit 7", "../orphan-pid"))]);
    let child = fixture.command().stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let (code, elapsed) = finishes_within(child, 15);
    if let Ok(pid) = fs::read_to_string(fixture.path("orphan-pid")) {
        let _ = Command::new("kill").arg(pid.trim()).status();
    }
    assert_eq!(code, Some(7), "a dead land check left the runner waiting on its orphaned log children for {elapsed:?}");
}

#[test]
fn hung_check_timeout() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: sleep 40")]);
    let output = fixture.command().env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(124), "{}", text(&output));
    assert!(text(&output).contains("exited 124"));
}

#[test]
fn frozen_check_timeout() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: touch ../started; sleep 40")]);
    let leases = fixture.path("runtime/agent-capacity-v1/leases");
    fs::create_dir_all(&leases).unwrap();
    fs::write(leases.join("frozen.json"), "{\"expiresAt\":1000,\"frozenAt\":1000}\n").unwrap();
    let mut child = fixture.command().env("AGENT_CAPACITY_LEASE", "frozen").env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    wait_for(&fixture.path("started"));
    std::thread::sleep(Duration::from_secs(4));
    assert!(child.try_wait().unwrap().is_none(), "a frozen land check was ended by its wall-clock timeout");
    fs::write(leases.join("frozen.json"), "{\"expiresAt\":5000}\n").unwrap();
    std::thread::sleep(Duration::from_millis(1600));
    assert!(child.try_wait().unwrap().is_none(), "a frozen land check ended before its timeout plus the time it was frozen");
    let (code, _) = finishes_within(child, 10);
    assert_eq!(code, Some(124), "a thawed land check outlived its timeout plus the time it was frozen");
}

#[test]
fn lease_around_local_check() {
    let check = r#"[ -e ../held ] && touch ../check-held; touch ../check-started; read -r _ <../check-release"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {check}"))]);
    let release = fixture.path("check-release");
    assert!(Command::new("mkfifo").arg(&release).status().unwrap().success());
    let child = fixture.with_lease().stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    wait_for(&fixture.path("check-started"));
    assert!(fixture.path("check-held").exists(), "the land check ran without the lease");
    assert!(fixture.path("held").exists());
    fs::write(&release, "go\n").unwrap();
    let (code, _) = finishes_within(child, 20);
    assert_eq!(code, Some(0));
    assert!(!fixture.path("held").exists(), "the lease outlived the local land check");
    assert_eq!(fixture.lease_events(), ["acquire", "release"]);
}

#[test]
fn two_local_checks_take_one_lease() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: [ -e ../held ] && printf one >>../order"), ("land.check", "file.txt :: [ -e ../held ] && printf two >>../order")]);
    let output = fixture.with_lease().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert_eq!(fixture.lease_events(), ["acquire", "release"], "two local checks must share one lease");
    assert_eq!(fs::read_to_string(fixture.path("order")).unwrap(), "onetwo");
}

#[test]
fn actions_only_run_takes_no_lease() {
    let fixture = Fixture::new(&[("land.workflow", "land-check.yml"), ("land.check", "file.txt :: true")]);
    let output = fixture.with_lease().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.lease_events().is_empty(), "an Actions-only run took a lease: {:?}", fixture.lease_events());
    assert!(fixture.trace().contains("land-actions --workflow land-check.yml"));
}

#[test]
fn lease_released_for_actions_wait() {
    let local = "[ -e ../held ] && touch ../local-held; held=$?; touch ../local-ran; exit $held";
    let fixture = Fixture::new(&[
        ("land.workflow", "land-check.yml"),
        ("land.check", &format!("file.txt :: {local}")),
        ("land.check", "file.txt :: true"),
        ("land.local-only", &format!("needs this host :: {local}")),
    ]);
    let release = fixture.path("actions-release");
    assert!(Command::new("mkfifo").arg(&release).status().unwrap().success());
    let child = fixture.with_lease().env("STUB_ACTIONS_RELEASE", &release).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    wait_for(&fixture.path("actions-release.waiting"));
    wait_for(&fixture.path("local-ran"));
    assert!(fixture.path("local-held").exists(), "the local land check ran without the lease");
    let start = Instant::now();
    while fixture.path("held").exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "the lease was held through the GitHub Actions wait");
        std::thread::sleep(Duration::from_millis(20));
    }
    fs::write(&release, "go\n").unwrap();
    let (code, _) = finishes_within(child, 20);
    assert_eq!(code, Some(0));
    assert_eq!(fixture.lease_events(), ["acquire", "release"]);
}

#[test]
fn admitted_check_unlocked() {
    let check = r#"found=$(for descriptor in /proc/$$/fd/*; do readlink "$descriptor" || true; done); printf "%s\n" "$found" >../descriptors"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {check}"))]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let descriptors = fs::read_to_string(fixture.path("descriptors")).unwrap();
    for target in descriptors.lines() {
        assert!(target.starts_with("pipe:") || target.starts_with("/dev/") || target.starts_with("/proc/"), "a land check inherited {target}");
    }
}

#[test]
fn land_check_retry() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: printf 'run\\n' >>../runs")]);
    for _ in 0..2 {
        let output = fixture.command().output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    }
    assert_eq!(fs::read_to_string(fixture.path("runs")).unwrap().lines().count(), 2);
    assert!(!fixture.trace().contains("land-clean"), "a land check rerun released the lane build output");
}

#[test]
fn rust_candidate_builds_north_only_on_the_build_host() {
    let mut fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    fs::write(fixture.repo.join(".safe-push"), include_str!("../../.safe-push")).unwrap();
    for path in ["crates/north-core/src/lib.rs", "src/cli/mod.rs", "flake.nix", "docs/features.md"] {
        let file = fixture.repo.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "changed\n").unwrap();
    }
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-qm", "rust candidate"]);
    fixture.head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    script(&fixture.path("stubs/cargo"), "printf 'cargo %s\\n' \"$*\" >>\"$STUB_TRACE\"\nexit 1\n");
    let output = fixture.with_lease().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    let safe_push = fixture.repo.join(".safe-push");
    let entries = |key: &str| -> Vec<String> {
        let output = Command::new("git").args(["config", "--file"]).arg(&safe_push).args(["--get-all", key]).output().unwrap();
        String::from_utf8(output.stdout).unwrap().lines().filter_map(|entry| entry.split_once(" :: ").map(|(_, command)| command.to_owned())).collect()
    };
    assert!(entries("land.local-only").is_empty(), "a north land check is pinned to the lander host: {:?}", entries("land.local-only"));
    let bound = entries("land.machine-bound");
    assert!(bound.iter().any(|command| command.contains("citation-check.sh")) && bound.iter().any(|command| command.contains("check-owners.sh")) && bound.iter().any(|command| command.contains("land-checks.sh machine-bound")));
    for command in &bound {
        let offloaded = format!("capacity offload run --host buildbox --base {} --head {} ", fixture.base, fixture.head);
        assert!(trace.lines().any(|line| line.contains(&offloaded) && line.ends_with(&format!("-- {command}"))), "{command} did not run on the build host:\n{trace}");
    }
    let started = String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("safe-push: land check: ")).count();
    let offloaded = trace.lines().filter(|line| line.starts_with("north capacity offload run --host buildbox ")).count();
    assert!(started > bound.len() && started == offloaded, "{started} land checks started, {offloaded} on the build host:\n{trace}");
    assert!(!trace.contains("land-actions"), "{trace}");
    assert!(!trace.lines().any(|line| line.starts_with("cargo ")), "a land check ran cargo on the lander host:\n{trace}");
    assert!(!text(&output).contains("runs on this host because"), "{}", text(&output));
    assert!(fixture.lease_events().is_empty(), "the lander took a local capacity lease: {:?}", fixture.lease_events());
}
