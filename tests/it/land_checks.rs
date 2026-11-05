use north_core::foundation::shim;
use north_landing::landing::check_timings::{Basis, Timing, Timings};
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
      "offload run") [ -z "${STUB_OFFLOAD_EXEC:-}" ] || exec bash -euo pipefail -c "${!#}"; exit 0 ;;
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
        fs::write(root.path().join("loadavg"), "0.50 0.50 0.50 1/100 1\n").unwrap();
        Self { root, repo, base, head }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn command(&self) -> Command {
        self.command_between(&self.base, &self.head)
    }

    fn command_between(&self, base: &str, head: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        command
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", base, "--head", head, "--safe-push"])
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
            .env("AGENT_CAPACITY_LOADAVG", self.path("loadavg"))
            .env("AGENT_CAPACITY_CORES", "4")
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

fn staged(fixture: &Fixture, destination: &str) -> Output {
    let mut command = fixture.command_between(&fixture.base, "HEAD");
    command.args(["--destination", destination, "--wait-event", "{\"stage\":\"land checks\",\"status\":\"wait\"}", "--event-log"]).arg(fixture.path("state/safe-push/events.jsonl"));
    command.output().unwrap()
}

#[test]
fn land_checks_stage_starts_only_for_main_with_checks() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: printf ran >> check-runs")]);
    let events = fixture.path("state/safe-push/events.jsonl");
    let output = staged(&fixture, "topic");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(!events.exists() && !fixture.repo.join("check-runs").exists(), "a non-main destination started the land checks");

    let output = staged(&fixture, "main");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert_eq!(fs::read_to_string(&events).unwrap(), "{\"stage\":\"land checks\",\"status\":\"wait\"}\n");
    assert_eq!(fs::read_to_string(fixture.repo.join("check-runs")).unwrap(), "ran");

    let unchecked = Fixture::new(&[("land.inferred", "false")]);
    let output = staged(&unchecked, "main");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(!unchecked.path("state/safe-push/events.jsonl").exists(), "a tree without land checks recorded a land-checks wait");
}

#[test]
fn land_checks_refuse_a_symlinked_stale_install() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: printf ran >> check-runs")]);
    fs::write(fixture.repo.join("bun.lock"), "{}\n").unwrap();
    git(&fixture.repo, &["add", "bun.lock"]);
    git(&fixture.repo, &["commit", "-qm", "lock"]);
    let shared = fixture.path("shared-modules");
    fs::create_dir_all(&shared).unwrap();
    std::os::unix::fs::symlink(&shared, fixture.repo.join("node_modules")).unwrap();
    let output = staged(&fixture, "main");
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    let expected = format!(
        "safe-push: {}/node_modules is a symlink to {} and bun.lock no longer matches it — bun install would rewrite that checkout; replace the symlink with its own install; NOT pushed.\n",
        fs::canonicalize(&fixture.repo).unwrap().display(),
        shared.display()
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr), expected);
    assert_eq!(fs::read_to_string(fixture.path("outcome")).unwrap(), "skip\tskipped: bun install failed\n");
    assert!(!fixture.repo.join("check-runs").exists(), "the land checks ran on a stale install");
}

fn hung_log(fixture: &Fixture) -> String {
    let logs = fixture.logs();
    assert_eq!(logs.len(), 1, "{logs:?}");
    fs::read_to_string(&logs[0]).unwrap()
}

#[test]
fn hung_check_timeout() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: bash ../step.sh")]);
    fs::write(fixture.path("step.sh"), "printf 'step started\\n'\nsleep 600\n").unwrap();
    let output = fixture.command().env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "a hang on a lane whose base tree was never timed is infrastructure: {}", text(&output));
    let log = hung_log(&fixture);
    assert!(log.contains("bash -euo pipefail -c bash ../step.sh exceeded its 2 s deadline; still running:"), "{log}");
    assert!(log.lines().any(|line| line.trim_start().ends_with(" sleep 600")), "the deadline did not name the planted step: {log}");
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(outcome.contains("skip\tland check bash ../step.sh ran past its 2 s deadline and base tree "), "{outcome}");
    assert!(outcome.contains(" has never been timed, so the hang is infrastructure, not this lane"), "{outcome}");
}

#[test]
fn hung_check_judged_against_base() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: bash ../step.sh")]);
    fs::write(fixture.path("step.sh"), "exit 0\n").unwrap();
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    fs::write(fixture.repo.join("file.txt"), "lane\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "lane"]);
    let lane = git(&fixture.repo, &["rev-parse", "HEAD"]);
    fs::write(fixture.path("step.sh"), "sleep 600\n").unwrap();
    let output = fixture.command_between(&fixture.head, &lane).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(124), "a hang where the base tree finished the same check refuses the lane: {}", text(&output));
    assert!(text(&output).contains("safe-push: land check bash ../step.sh hung: it ran past its 2 s deadline on this lane, while base tree "), "{}", text(&output));
    let log = hung_log(&fixture);
    assert!(log.lines().any(|line| line.trim_start().ends_with(" sleep 600")), "{log}");
    assert!(!fs::read_to_string(fixture.path("outcome")).unwrap().contains("skip\t"));

    fs::write(fixture.repo.join("file.txt"), "again\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "next"]);
    let next = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&lane, &next).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "a base tree that also hung is no evidence against the lane: {}", text(&output));
    assert!(text(&output).contains(" also ran past it, so the hang is infrastructure, not this lane"), "{}", text(&output));
}

#[test]
fn hung_check_on_a_loaded_host_is_infrastructure() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: bash ../step.sh")]);
    fs::write(fixture.path("step.sh"), "exit 0\n").unwrap();
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    fs::write(fixture.repo.join("file.txt"), "lane\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "lane"]);
    let lane = git(&fixture.repo, &["rev-parse", "HEAD"]);
    fs::write(fixture.path("step.sh"), "sleep 600\n").unwrap();

    fs::write(fixture.path("loadavg"), "85.00 60.00 40.00 90/2000 1\n").unwrap();
    let output = fixture.command_between(&fixture.head, &lane).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "a hang at load 85 on 4 cores, where the base tree passed, requeues the lane: {}", text(&output));
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(outcome.contains(" while this host ran at load 85.0 over its 4 cores, so the hang is infrastructure, not this lane"), "{outcome}");

    fs::write(fixture.path("loadavg"), "3.00 2.00 1.00 3/2000 1\n").unwrap();
    let output = fixture.command_between(&fixture.head, &lane).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(124), "the same hang at load 3 on 4 cores refuses the lane: {}", text(&output));
    assert!(text(&output).contains("safe-push: land check bash ../step.sh hung: it ran past its 2 s deadline on this lane, while base tree "), "{}", text(&output));
}

#[test]
fn measured_deadline_ends_a_hang() {
    let check = "bash ../step.sh";
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {check}"))]);
    fs::write(fixture.path("step.sh"), "touch ../started\nsleep 600\n").unwrap();
    let timings = Timings::at(fixture.path("state/safe-push/check-timings"));
    for _ in 0..20 {
        timings.passed(check, "local", 60).unwrap();
    }
    let base_tree = git(&fixture.repo, &["rev-parse", &format!("{}^{{tree}}", fixture.base)]);
    timings.record(&base_tree, check, "local", Timing::Finished(60)).unwrap();
    let clock = fixture.path("clock");
    fs::create_dir_all(&clock).unwrap();
    let now = |millis: u64| fs::write(clock.join("now"), millis.to_string()).unwrap();
    let seen = |millis: u64| {
        let start = Instant::now();
        loop {
            let acknowledged = fs::read_dir(&clock).unwrap().flatten().filter(|entry| entry.file_name().to_string_lossy().starts_with("seen.")).any(|entry| fs::read_to_string(entry.path()).is_ok_and(|text| text == millis.to_string()));
            if acknowledged {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(30), "the deadline never read {millis} ms");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    now(0);
    let stdout = fs::File::create(fixture.path("stdout")).unwrap();
    let stderr = fs::File::create(fixture.path("stderr")).unwrap();
    let child = fixture.command().env("NORTH_CAPACITY_DEADLINE_CLOCK", &clock).stdout(stdout).stderr(stderr).spawn().unwrap();
    wait_for(&fixture.path("started"));
    now(299_000);
    seen(299_000);
    let mut child = child;
    assert!(child.try_wait().unwrap().is_none(), "a check measured at a 60 s p90 ended before its 300 s floor");
    now(300_000);
    let (code, _) = finishes_within(child, 20);
    let out = fs::read_to_string(fixture.path("stdout")).unwrap();
    let err = fs::read_to_string(fixture.path("stderr")).unwrap();
    assert_eq!(code, Some(124), "a hang past the measured deadline, where the base tree passed in 60 s, refuses the lane: {out}{err}");
    assert!(out.contains(&format!("safe-push: land check: {check} (deadline 300 s from p90 60 s over 20 passes)")), "{out}");
    assert!(err.contains(&format!("safe-push: land check {check} hung: it ran past its 300 s deadline on this lane, while base tree ")), "{err}");
    assert!(hung_log(&fixture).contains("exceeded its 300 s deadline; still running:"));
}

#[test]
fn unmeasured_deadline_names_its_passes() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    let timings = Timings::at(fixture.path("state/safe-push/check-timings"));
    for _ in 0..3 {
        timings.passed("true", "local", 1).unwrap();
    }
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(text(&output).contains("safe-push: land check: true (deadline 2100 s, 3 passes recorded)"), "{}", text(&output));
    assert_eq!(timings.deadline("true", "local").basis, Basis::Unmeasured { passes: 4 }, "a pass is recorded");
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
    assert_eq!(code, Some(75), "a thawed land check outlived its timeout plus the time it was frozen");
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
    let checks = entries("land.check");
    let bound: Vec<String> = entries("land.machine-bound").into_iter().filter(|command| checks.contains(command)).collect();
    assert!(bound.iter().any(|command| command.contains("citation-check.sh")) && bound.iter().any(|command| command.contains("check-owners.sh")));
    for command in &bound {
        let offloaded = format!("capacity offload run --host buildbox --base {} --head {} ", fixture.base, fixture.head);
        assert!(trace.lines().any(|line| line.contains(&offloaded) && line.ends_with(&format!("-- {command}"))), "{command} did not run on the build host:\n{trace}");
    }
    let started = String::from_utf8_lossy(&output.stdout).lines().filter(|line| line.starts_with("safe-push: land check: ")).count();
    let offloaded = trace.lines().filter(|line| line.starts_with("north capacity offload run --host buildbox ")).count();
    assert!(started > bound.len() && started == offloaded, "{started} land checks started, {offloaded} on the build host:\n{trace}");
    assert!(!trace.contains("land-actions"), "{trace}");
    assert!(!trace.lines().any(|line| line.starts_with("cargo ") && !line.starts_with("cargo metadata --no-deps ")), "a land check ran cargo on the lander host:\n{trace}");
    assert!(!text(&output).contains("runs on this host because"), "{}", text(&output));
    assert!(fixture.lease_events().is_empty(), "the lander took a local capacity lease: {:?}", fixture.lease_events());
}

const NIX_STUB: &str = "printf 'nix %s\\n' \"$*\" >>\"$STUB_TRACE\"\nwhile [ \"$#\" -gt 0 ] && [ \"$1\" != -c ]; do shift; done\nshift\nexec \"$@\"\n";
const TOOL_STUB: &str = "printf '%s %s\\n' \"$(basename \"$0\")\" \"$*\" >>\"$STUB_TRACE\"\nexit \"${STUB_TOOL_STATUS:-0}\"\n";
const DEV_SHELL_FLAKE: &str = "{ outputs = { self }: { devShells.x86_64-linux.default = 1; }; }\n";

impl Fixture {
    fn tree(settings: &[(&str, &str)], files: &[(&str, &str)]) -> Self {
        let mut fixture = Self::new(settings);
        for (name, body) in files {
            fs::write(fixture.repo.join(name), body).unwrap();
            git(&fixture.repo, &["add", name]);
        }
        git(&fixture.repo, &["commit", "-qm", "tree"]);
        fixture.head = git(&fixture.repo, &["rev-parse", "HEAD"]);
        for tool in ["cargo", "bun"] {
            script(&fixture.path(&format!("stubs/{tool}")), TOOL_STUB);
        }
        script(&fixture.path("stubs/nix"), NIX_STUB);
        fixture
    }
}

fn which(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap()).map(|directory| directory.join(name)).find(|path| path.is_file()).unwrap()
}

#[test]
fn inferred_checks_run_in_the_dev_shell_of_a_repository_without_land_check_lines() {
    let fixture = Fixture::tree(&[("land.inferred", "true")], &[("Cargo.toml", "[package]\n"), ("flake.nix", DEV_SHELL_FLAKE)]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let log = text(&output);
    assert!(log.contains("no land.check lines in .safe-push; running the 2 land check(s) inferred from the tree"), "{log}");
    assert_eq!(log.matches("safe-push: land check (inferred): ").count(), 2, "{log}");
    let trace: Vec<String> = fixture.trace().lines().filter(|line| line.starts_with("nix ") || line.starts_with("cargo ")).map(str::to_owned).collect();
    assert_eq!(trace.len(), 4, "{trace:?}");
    assert!(trace[0].starts_with("nix develop --offline --no-update-lock-file -c bash -euc ") && trace[0].ends_with("cargo test --locked --offline"), "{trace:?}");
    assert_eq!(trace[1], "cargo test --locked --offline");
    assert!(trace[2].starts_with("nix develop ") && trace[2].contains("cargo clippy --all-targets --locked --offline -- -D warnings"), "{trace:?}");
    assert_eq!(trace[3], "cargo clippy --all-targets --locked --offline -- -D warnings");
}

#[test]
fn inferred_checks_without_a_flake_run_the_tool_directly_and_in_a_fixed_order() {
    let fixture = Fixture::tree(&[("land.inferred", "true")], &[("Cargo.toml", "[package]\n"), ("package.json", "{}\n"), ("bun.lock", "\n")]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    let runs: Vec<&str> = trace.lines().filter(|line| line.starts_with("cargo ") || line.starts_with("bun ")).collect();
    assert_eq!(runs, ["cargo test --locked --offline", "cargo clippy --all-targets --locked --offline -- -D warnings", "bun install --frozen-lockfile", "bun test"]);
    assert!(!trace.contains("nix "), "{trace}");
}

#[test]
fn explicit_land_check_lines_replace_inference() {
    let fixture = Fixture::tree(&[("land.check", "file.txt :: touch ../explicit")], &[("Cargo.toml", "[package]\n")]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.path("explicit").exists());
    assert!(!fixture.trace().contains("cargo "), "{}", fixture.trace());
    assert!(!text(&output).contains("inferred"), "{}", text(&output));
}

#[test]
fn land_inferred_false_turns_inference_off() {
    let fixture = Fixture::tree(&[("land.inferred", "false")], &[("Cargo.toml", "[package]\n")]);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(!fixture.trace().contains("cargo "), "{}", fixture.trace());
    assert!(!text(&output).contains("inferred"), "{}", text(&output));
}

#[test]
fn an_inferred_check_fails_the_landing_with_its_exit_status() {
    let fixture = Fixture::tree(&[("land.inferred", "true")], &[("Cargo.toml", "[package]\n")]);
    let output = fixture.command().env("STUB_TOOL_STATUS", "101").output().unwrap();
    assert_eq!(output.status.code(), Some(101), "{}", text(&output));
    assert!(text(&output).contains("NOT pushed"), "{}", text(&output));
}

#[test]
fn a_missing_toolchain_is_an_infrastructure_exit() {
    let fixture = Fixture::tree(&[("land.inferred", "true")], &[("Cargo.toml", "[package]\n")]);
    let bare = fixture.path("bare");
    fs::create_dir_all(&bare).unwrap();
    for tool in ["bash", "git"] {
        std::os::unix::fs::symlink(which(tool), bare.join(tool)).unwrap();
    }
    let output = fixture.command().env("PATH", &bare).output().unwrap();
    assert_eq!(output.status.code(), Some(75), "{}", text(&output));
}

#[test]
fn safe_push_counts_inferred_checks_as_land_checks() {
    let probe = |settings: &[(&str, &str)], files: &[(&str, &str)]| -> Option<i32> {
        let fixture = Fixture::tree(settings, files);
        let safe_push = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push");
        let script = format!("source <(sed -n '/^has_land_checks() {{/,/^}}/p' '{}'); has_land_checks", safe_push.display());
        let path = format!("{}:{}", north_dir(), std::env::var("PATH").unwrap_or_default());
        Command::new("bash").args(["-c", &script]).current_dir(&fixture.repo).env("PATH", path).status().unwrap().code()
    };
    assert_eq!(probe(&[("land.inferred", "true")], &[("Cargo.toml", "[package]\n")]), Some(0));
    assert_eq!(probe(&[("land.inferred", "true")], &[("README.md", "x\n")]), Some(1));
    assert_eq!(probe(&[("land.inferred", "false")], &[("Cargo.toml", "[package]\n")]), Some(1));
    assert_eq!(probe(&[("land.check", "file.txt :: true")], &[("README.md", "x\n")]), Some(0));
}

fn north_dir() -> String {
    Path::new(env!("CARGO_BIN_EXE_north")).parent().unwrap().to_string_lossy().into_owned()
}

const ENVIRONMENT_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>../runs; [ "$SAFE_PUSH_LAND_HEAD" = "$(git rev-parse lane)" ] || exit 9; [ ! -e ../plant ] || { echo "bash: nix: command not found" >&2; exit 127; }; if grep -q broken file.txt; then echo "error: the lane broke file.txt" >&2; exit 1; fi"#;

fn lane_onto(fixture: &Fixture, text: &str) -> String {
    git(&fixture.repo, &["checkout", "-q", "-B", "lane", &fixture.head]);
    fs::write(fixture.repo.join("file.txt"), text).unwrap();
    git(&fixture.repo, &["commit", "-qam", "lane"]);
    git(&fixture.repo, &["rev-parse", "HEAD"])
}

fn runs(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(fixture.path("runs")).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn failed_check_judged_against_base() {
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {ENVIRONMENT_CHECK}"))]);
    let lane = lane_onto(&fixture, "lane\n");
    fs::write(fixture.path("plant"), "").unwrap();
    let output = fixture.command_between(&fixture.head, &lane).env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "a check that also fails on the base tree is the environment, not the lane: {}", text(&output));
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(outcome.lines().any(|line| line.starts_with("skip\tmain's check environment is broken: ") && line.ends_with(": bash: nix: command not found")), "{outcome}");
    assert_eq!(runs(&fixture), [lane.clone(), fixture.head.clone()], "the base tree did not rerun the failed check");
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]), lane, "the base rerun left the worktree off the lane");
    assert_eq!(git(&fixture.repo, &["symbolic-ref", "--short", "HEAD"]), "lane");

    let output = fixture.command_between(&fixture.head, &lane).env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "{}", text(&output));
    assert!(text(&output).contains("already ran land check"), "{}", text(&output));
    assert_eq!(runs(&fixture).len(), 3, "one round reran the base check twice: {:?}", runs(&fixture));

    let output = fixture.command_between(&fixture.head, &lane).env("NORTH_LAND_ROUND", "r2").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "{}", text(&output));
    assert_eq!(runs(&fixture).len(), 5, "a new round reuses no base result: {:?}", runs(&fixture));

    let output = fixture.command_between(&fixture.head, &lane).output().unwrap();
    assert_eq!(output.status.code(), Some(127), "outside a lander round the failure stands: {}", text(&output));
    assert_eq!(runs(&fixture).len(), 6);

    fs::remove_file(fixture.path("plant")).unwrap();
    fs::remove_file(fixture.path("outcome")).unwrap();
    let broken = lane_onto(&fixture, "broken\n");
    let output = fixture.command_between(&fixture.head, &broken).env("NORTH_LAND_ROUND", "r3").output().unwrap();
    assert_eq!(output.status.code(), Some(1), "a check the base tree passes refuses the lane: {}", text(&output));
    assert!(!fs::read_to_string(fixture.path("outcome")).unwrap().contains("environment is broken"));
    assert_eq!(runs(&fixture)[6..], [broken.clone(), fixture.head.clone()]);
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]), broken);
}

#[test]
fn check_a_lane_adds_is_not_judged_against_base() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: printf x >>../runs; exit 127")]);
    let output = fixture.command().env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(127), "{}", text(&output));
    assert_eq!(fs::read_to_string(fixture.path("runs")).unwrap(), "x", "a check the base tree does not list ran on it");
}

const LIVE_NORTH: &str = "printf 'live-north %s\\n' \"$*\" >>\"$STUB_TRACE\"\n[ \"$1 $2\" != \"runtime covers\" ] || exec \"$REAL_NORTH\" \"$@\"\n";
const TREE_NORTH: &str = "#!/usr/bin/env bash\nprintf 'tree-north %s\\n' \"$*\" >>\"$STUB_TRACE\"\n";
const LAND_SHELL: &str = "printf 'land-shell %s\\n' \"$*\" >>\"$STUB_TRACE\"\ncase \"$*\" in *\"cargo build\"*) mkdir -p target/debug && printf '%s' \"$TREE_NORTH\" >target/debug/north && chmod +x target/debug/north ;; esac\n";

impl Fixture {
    fn north_candidate(changed: &[&str]) -> Self {
        let mut fixture = Self::new(&[("land.check", "file.txt :: true")]);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"));
        let settings: String = include_str!("../../.safe-push").lines().filter(|line| !line.starts_with("\tworkflow =") && !line.starts_with("\trunner =")).map(|line| format!("{line}\n")).collect();
        fs::write(fixture.repo.join(".safe-push"), settings).unwrap();
        for path in ["bin/head-north", "harness/scripts/citation-check.sh", "harness/scripts/check-owners.sh"] {
            let file = fixture.repo.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::copy(source.join(path), file).unwrap();
        }
        script(&fixture.repo.join("bin/land-shell"), LAND_SHELL);
        fs::write(fixture.repo.join("harness/machine-bound-tests"), "").unwrap();
        for directory in ["lib", "harness/hooks", "harness/lib"] {
            fs::create_dir_all(fixture.repo.join(directory)).unwrap();
            fs::write(fixture.repo.join(directory).join("keep"), "").unwrap();
        }
        git(&fixture.repo, &["add", "-A"]);
        git(&fixture.repo, &["commit", "-qm", "north tree"]);
        fixture.base = git(&fixture.repo, &["rev-parse", "HEAD"]);
        for path in changed {
            let file = fixture.repo.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "changed\n").unwrap();
        }
        git(&fixture.repo, &["add", "-A"]);
        git(&fixture.repo, &["commit", "-qm", "candidate"]);
        fixture.head = git(&fixture.repo, &["rev-parse", "HEAD"]);
        let runtime = fixture.path(&format!("state/north/runtimes/{}", fixture.base));
        fs::create_dir_all(&runtime).unwrap();
        script(&runtime.join("north"), LIVE_NORTH);
        std::os::unix::fs::symlink(format!("runtimes/{}", fixture.base), fixture.path("state/north/current")).unwrap();
        script(&fixture.path("stubs/cargo"), "printf 'cargo %s\\n' \"$*\" >>\"$STUB_TRACE\"\nexit 1\n");
        fixture
    }

    fn north_checks(&self) -> (Output, Duration) {
        let started = Instant::now();
        let output = self.command().env("SAFE_PUSH_LAND_RUNNER", "").env("HOME", self.root.path()).env("TREE_NORTH", TREE_NORTH).output().unwrap();
        (output, started.elapsed())
    }
}

#[test]
fn a_candidate_with_no_rust_build_input_runs_its_checks_with_the_live_runtime_and_no_cargo() {
    let fixture = Fixture::north_candidate(&["docs/features.md", "docs/rationale/x.md", "harness/skills/workers/SKILL.md", "AGENTS.md"]);
    let (output, elapsed) = fixture.north_checks();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    assert!(!trace.lines().any(|line| line.starts_with("cargo ") || line.starts_with("land-shell ") || line.starts_with("tree-north ")), "a docs-only candidate built north:\n{trace}");
    for run in ["citation-check --root", "config agents check-owners", "docs features-index --check", "docs rationale-index --check", "agent-config-check tree-policy"] {
        assert!(trace.lines().any(|line| line.starts_with(&format!("live-north {run}"))), "{run} did not run on the live runtime's north:\n{trace}");
    }
    assert!(elapsed < Duration::from_secs(30), "the docs-only checks took {elapsed:?}");
    println!("docs-only candidate checks finished in {elapsed:?}");
}

#[test]
fn a_rust_candidate_compiles_once_for_clippy_and_builds_north_only_for_a_check_whose_code_it_changes() {
    let fixture = Fixture::north_candidate(&["crates/north-core/src/lib.rs", "docs/features.md", "docs/rationale/x.md"]);
    let (output, _) = fixture.north_checks();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = fixture.trace();
    assert!(trace.lines().any(|line| line.starts_with("land-shell --dev -- cargo clippy --all-targets")), "clippy did not run:\n{trace}");
    assert!(!trace.lines().any(|line| line.contains("cargo build") || line.starts_with("tree-north ")), "a Rust candidate that changes no check's code built north:\n{trace}");
    for run in ["docs features-index --check", "docs rationale-index --check", "citation-check --root", "config agents check-owners"] {
        assert!(trace.lines().any(|line| line.starts_with(&format!("live-north {run}"))), "{run} did not run on the live runtime's north:\n{trace}");
    }
    let indexer = Fixture::north_candidate(&["crates/north-ops/src/ops/features_index/mod.rs", "docs/features.md", "docs/rationale/x.md"]);
    let (output, _) = indexer.north_checks();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = indexer.trace();
    assert!(trace.lines().any(|line| line.starts_with("land-shell --dev -- cargo build -q --locked --offline --bin north")), "{trace}");
    assert!(trace.lines().any(|line| line.starts_with("tree-north docs features-index --check")), "a changed features indexer ran on the live runtime:\n{trace}");
    assert!(trace.lines().any(|line| line.starts_with("live-north docs rationale-index --check")), "{trace}");
    let embedded = Fixture::north_candidate(&["harness/model-catalog.toml"]);
    let (output, _) = embedded.north_checks();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let trace = embedded.trace();
    assert!(trace.lines().any(|line| line.starts_with("land-shell --dev -- cargo clippy --all-targets")), "an embedded Rust input skipped clippy:\n{trace}");
}

const COVERING_NORTH: &str = "[ \"$1 $2\" != \"runtime covers\" ] || { printf '%s\\n' \"$REAL_NORTH\"; exit 0; }\nexec \"$REAL_NORTH\" \"$@\"\n";
const USAGE_RS: &str = "crates/north-core/src/usage/mod.rs";

fn layers_lane(usage: &str) -> (Fixture, Output) {
    let mut fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    let source = Path::new(env!("CARGO_MANIFEST_DIR"));
    let layers: Vec<&str> = include_str!("../../.safe-push").lines().filter(|line| line.contains(" code-health layers ")).collect();
    assert_eq!(layers.len(), 1, "{layers:?}");
    fs::write(fixture.repo.join(".safe-push"), format!("[land]\n{}\n", layers[0])).unwrap();
    let files = [
        ("crates/north-core/Cargo.toml", "[package]\nname = \"north-core\"\nversion = \"0.1.0\"\n"),
        ("crates/north-core/src/lib.rs", "pub mod fleet;\npub mod usage;\n"),
        ("crates/north-core/src/fleet/mod.rs", "pub mod records;\n"),
        ("crates/north-core/src/fleet/records.rs", "pub fn read_record() {}\n"),
        (USAGE_RS, "pub fn read() {}\n"),
    ];
    for (path, body) in files {
        fs::create_dir_all(fixture.repo.join(path).parent().unwrap()).unwrap();
        fs::write(fixture.repo.join(path), body).unwrap();
    }
    for path in ["bin/head-north", "structure.toml"] {
        fs::create_dir_all(fixture.repo.join(path).parent().unwrap()).unwrap();
        fs::copy(source.join(path), fixture.repo.join(path)).unwrap();
    }
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-qm", "north layout"]);
    fixture.base = git(&fixture.repo, &["rev-parse", "HEAD"]);
    fs::write(fixture.repo.join(USAGE_RS), usage).unwrap();
    git(&fixture.repo, &["commit", "-qam", "usage lane"]);
    fixture.head = git(&fixture.repo, &["rev-parse", "HEAD"]);
    fs::create_dir_all(fixture.path("state/north/current")).unwrap();
    script(&fixture.path("state/north/current/north"), COVERING_NORTH);
    let output = fixture.command().output().unwrap();
    (fixture, output)
}

#[test]
fn a_lane_adding_crate_fleet_to_usage_is_refused_at_landing_naming_the_rule() {
    let (fixture, output) = layers_lane("use crate::fleet::records::read_record;\n\npub fn read() {\n    read_record();\n}\n");
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(String::from_utf8_lossy(&output.stderr).contains("code-health layers") && text(&output).contains("NOT pushed."), "{}", text(&output));
    let logs = fixture.logs();
    assert_eq!(logs.len(), 1, "{logs:?}");
    let log = fs::read_to_string(&logs[0]).unwrap();
    assert!(log.contains(&format!("✗ layer {USAGE_RS}:1 new reference -> crates/north-core/src/fleet/records.rs: module usage may not use fleet")), "{log}");
    assert!(log.contains(&format!("✗ crate-dag {USAGE_RS}:1 new reference -> crates/north-core/src/fleet/records.rs: planned crate north-pace may not use north-fleet")), "{log}");

    let (fixture, output) = layers_lane("pub fn read() -> u8 {\n    1\n}\n");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.logs().is_empty(), "a passing layers check kept its log");
}

fn seed_outcomes(fixture: &Fixture, rows: &[(&str, &str, f64)]) -> PathBuf {
    let path = fixture.path("state/safe-push/check-timings/outcomes.tsv");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lines: String = rows
        .iter()
        .enumerate()
        .map(|(index, (command, outcome, cpu))| format!("{index}\tseed-{}\tseed\t{outcome}\t{}\t{}\t{cpu}\t{command}\n", index / 2, i32::from(*outcome == "failed"), cpu / 4.0))
        .collect();
    fs::write(&path, lines).unwrap();
    path
}

#[test]
fn expensive_land_checks_run_in_fail_fast_order_and_are_cancelled_at_the_first_failure() {
    let (cheap, likely, unlikely) = ("touch ../cheap-ran", "touch ../likely-ran; exit 1", "touch ../unlikely-ran");
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {unlikely}")), ("land.check", &format!("file.txt :: {likely}")), ("land.check", &format!("file.txt :: {cheap}"))]);
    let mut seeded = Vec::new();
    for _ in 0..3 {
        seeded.extend([(likely, "failed", 200.0), (unlikely, "passed", 500.0)]);
    }
    let records = seed_outcomes(&fixture, &seeded);
    let output = fixture.command().output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains(&format!("2 expensive land check(s) run in fail-fast order and are cancelled at the first failure: {likely}; {unlikely}")), "{}", text(&output));
    assert!(fixture.path("cheap-ran").exists(), "a cheap check must always finish");
    assert!(fixture.path("likely-ran").exists());
    assert!(!fixture.path("unlikely-ran").exists(), "the expensive check after the first failure ran");
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(outcome.lines().any(|line| line == format!("event\tland check\tcancelled\t{unlikely}: an earlier land check failed")), "{outcome}");
    let written: Vec<String> = fs::read_to_string(records).unwrap().lines().skip(seeded.len()).map(|line| line.split('\t').skip(3).step_by(4).collect::<Vec<_>>().join(" ")).collect();
    assert_eq!(written, [format!("passed {cheap}"), format!("failed {likely}"), format!("cancelled {unlikely}")]);
}

#[test]
fn land_checks_replay_prints_cpu_before_the_failure_verdict_for_both_orders() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    let mut rows = Vec::new();
    for landing in 0..10 {
        rows.push(("suite", "passed", 400.0));
        rows.push(("build", if landing % 3 == 0 { "passed" } else { "failed" }, 200.0));
    }
    let records = seed_outcomes(&fixture, &rows);
    let output = fixture.command().args(["--replay", "--records"]).arg(&records).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let printed = text(&output);
    assert!(printed.contains("landings replayed: 6 failing, 4 passing"), "{printed}");
    assert!(printed.contains("median CPU-s before the failure verdict: 600.0 with today's order and cancel-on-first-failure, 200.0 with the fail-fast order"), "{printed}");
    assert!(printed.contains("median passing landing wall: 150.0 s today, 150.0 s with the fail-fast order (+0.0%)"), "{printed}");
}

const SCOPE_CHECK: &str = r#"printf '%s|%s\n' "${SAFE_PUSH_LAND_CPUS:-}" "${SAFE_PUSH_LAND_PACKAGES:---workspace}" >>../scopes"#;

fn workspace_lane(fixture: &Fixture, changed: &[&str]) -> (String, String) {
    let manifest = |name: &str, dependencies: &[(&str, &str)]| {
        let dependencies: String = dependencies.iter().map(|(dependency, path)| format!("{dependency} = {{ path = \"{path}\" }}\n")).collect();
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n{dependencies}")
    };
    for (name, dependencies) in [("north-base", vec![]), ("north-core", vec![("north-base", "../north-base")]), ("north-landing", vec![("north-base", "../north-base")])] {
        let directory = fixture.repo.join("crates").join(name);
        fs::create_dir_all(directory.join("src")).unwrap();
        fs::write(directory.join("Cargo.toml"), manifest(name, &dependencies)).unwrap();
        fs::write(directory.join("src/lib.rs"), "").unwrap();
    }
    fs::create_dir_all(fixture.repo.join("src")).unwrap();
    fs::write(fixture.repo.join("src/main.rs"), "fn main() {}\n").unwrap();
    let root = manifest("north", &[("north-core", "crates/north-core"), ("north-landing", "crates/north-landing")]);
    fs::write(fixture.repo.join("Cargo.toml"), format!("[workspace]\nmembers = [\"crates/north-base\", \"crates/north-core\", \"crates/north-landing\"]\n\n{root}")).unwrap();
    fs::write(fixture.repo.join(".gitignore"), "Cargo.lock\ntarget/\n").unwrap();
    git(&fixture.repo, &["checkout", "-q", "-B", "workspace", &fixture.head]);
    git(&fixture.repo, &["add", "-A"]);
    git(&fixture.repo, &["commit", "-qm", "workspace"]);
    let base = git(&fixture.repo, &["rev-parse", "HEAD"]);
    for path in changed {
        let file = fixture.repo.join(path);
        let text = fs::read_to_string(&file).unwrap_or_default();
        fs::write(file, format!("{text}\n")).unwrap();
    }
    git(&fixture.repo, &["add", "-A", "-f"]);
    git(&fixture.repo, &["commit", "-qm", "lane"]);
    (base, git(&fixture.repo, &["rev-parse", "HEAD"]))
}

fn scope_of(changed: &[&str]) -> String {
    let fixture = Fixture::new(&[("land.check", &format!("crates/,src/,Cargo.toml,Cargo.lock :: {SCOPE_CHECK}"))]);
    let (base, lane) = workspace_lane(&fixture, changed);
    let output = fixture.command_between(&base, &lane).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    fs::read_to_string(fixture.path("scopes")).unwrap()
}

#[test]
fn affected_crates_only() {
    assert_eq!(scope_of(&["crates/north-landing/src/lib.rs"]), "4|-p north -p north-landing\n", "a north-landing lane checks north-landing and the north binary crate, not north-core");
    assert_eq!(scope_of(&["crates/north-core/src/lib.rs"]), "4|-p north -p north-core\n");
    assert_eq!(scope_of(&["src/main.rs"]), "4|-p north\n");
    assert_eq!(scope_of(&["crates/north-base/src/lib.rs"]), "|--workspace\n", "every crate depends on north-base");
    assert_eq!(scope_of(&["crates/north-landing/src/lib.rs", "Cargo.toml"]), "|--workspace\n");
    assert_eq!(scope_of(&["crates/north-landing/src/lib.rs", "Cargo.lock"]), "|--workspace\n");
}

#[test]
fn every_offloaded_land_check_starts_together_whatever_its_fail_fast_rank() {
    let meet = |me: &str, other: &str| format!("touch ../started-{me}; for i in $(seq 100); do [ -e ../started-{other} ] && exit 0; sleep 0.1; done; exit 9");
    let (one, two) = (meet("one", "two"), meet("two", "one"));
    let fixture = Fixture::new(&[("land.runner", "build-host"), ("land.check", &format!("file.txt :: {one}")), ("land.check", &format!("file.txt :: {two}"))]);
    seed_outcomes(&fixture, &[(one.as_str(), "failed", 300.0), (two.as_str(), "passed", 600.0), (one.as_str(), "failed", 300.0), (two.as_str(), "passed", 600.0)]);
    let started = Instant::now();
    let output = fixture.command().env("STUB_OFFLOAD_EXEC", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "two offloaded checks waited for each other: {}", text(&output));
    assert!(text(&output).contains("2 expensive land check(s)"), "{}", text(&output));
    assert!(started.elapsed() < Duration::from_secs(9), "offloaded checks ran one after the other");
    assert!(fixture.trace().lines().filter(|line| line.starts_with("north capacity offload run")).count() == 2, "{}", fixture.trace());
}

#[test]
fn every_offloaded_land_check_starts_together_with_no_records() {
    let meet = |me: &str, other: &str| format!("touch ../started-{me}; for i in $(seq 100); do [ -e ../started-{other} ] && exit 0; sleep 0.1; done; exit 9");
    let fixture = Fixture::new(&[("land.runner", "build-host"), ("land.check", &format!("file.txt :: {}", meet("one", "two"))), ("land.check", &format!("file.txt :: {}", meet("two", "one")))]);
    let output = fixture.command().env("STUB_OFFLOAD_EXEC", "1").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "two offloaded checks waited for each other: {}", text(&output));
}
