use north_core::foundation::shim;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
const NORTH_STUB: &str = r#"printf 'north %s\n' "$*" >>"$STUB_TRACE"
case "$1" in
  capacity)
    case "$2 $3" in
      "offload choose") printf 'buildbox\n'; exit 0 ;;
      "offload verdict")
        [ -n "${STUB_VERDICTS:-}" ] || exit 0
        mkdir -p "$STUB_VERDICTS"
        if [ "$4" = hit ]; then
          shift 4
          for key in "$@"; do [ ! -f "$STUB_VERDICTS/$key" ] || printf '%s stubhost %s\n' "$key" "$(cat "$STUB_VERDICTS/$key")"; done
          exit 0
        fi
        if [ "$9" = --pass ]; then printf '1000 %s' "${10}" >"$STUB_VERDICTS/$8"; else rm -f "$STUB_VERDICTS/$8"; fi
        exit 0
        ;;
      "offload run") [ -z "${STUB_OFFLOAD_EXEC:-}" ] || exec bash -euo pipefail -c "${!#}"; exit 0 ;;
    esac
    ;;
  land)
    case "$2" in
      red-check) exit "${STUB_RED_STATUS:-0}" ;;
      actions)
        if [ -n "${STUB_ACTIONS_RELEASE:-}" ]; then
          touch "$STUB_ACTIONS_RELEASE.waiting"
          read -r _ <"$STUB_ACTIONS_RELEASE"
        fi
        exit "${STUB_ACTIONS_STATUS:-0}"
        ;;
    esac
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
        fs::create_dir_all(root.path().join("state/safe-push")).unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("state/safe-push/base-trees")).unwrap();
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
        self.command_through(base, head, &self.path("stubs/safe-push"))
    }

    fn command_through(&self, base: &str, head: &str, safe_push: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        command
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", base, "--head", head, "--safe-push"])
            .arg(safe_push)
            .arg("--outcome")
            .arg(self.path("outcome"))
            .env("PATH", path)
            .env("NORTH_LAND_NORTH", self.path("stubs/north"))
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("STUB_TRACE", self.path("trace"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_CACHE_HOME", self.path("cache"))
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
            .env_remove("NORTH_LAND_SPANS")
            .env_remove("SAFE_PUSH_LANDING")
            .env_remove("SAFE_PUSH_LAND_BASE")
            .env_remove("SAFE_PUSH_LAND_HEAD")
            .env_remove("SAFE_PUSH_LAND_RUNTIME_HEAD")
            .env_remove("SAFE_PUSH_LAND_REF")
            .env_remove("SAFE_PUSH_LAND_PACKAGES")
            .env_remove("SAFE_PUSH_LAND_CPUS")
            .env_remove("SAFE_PUSH_LAND_IT")
            .env_remove("SAFE_PUSH_LAND_SELECT_PATHS")
            .stdin(Stdio::null());
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
}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
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

    for (leaks, status) in [("0", 0), ("1", 1)] {
        let fixture = Fixture::new(&[("land.publish", "farm/"), ("land.check", "file.txt :: touch ../ran")]);
        git(&fixture.repo, &["config", "user.email", "t@example.invalid"]);
        let link = fixture.path("link/safe-push");
        fs::create_dir_all(fixture.path("link")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), &link).unwrap();
        assert_eq!(fs::canonicalize(&link).unwrap().file_name().unwrap(), "north");
        script(&fixture.path("stubs/betterleaks"), "printf 'betterleaks %s\\n' \"$*\" >>\"$STUB_TRACE\"\nexit \"$STUB_LEAKS_STATUS\"\n");
        let output = fixture.command_through(&fixture.base, &fixture.head, &link).env("HOME", fixture.path("home")).env_remove("SAFE_PUSH_DENYLIST").env_remove("SAFE_PUSH_SOUTH").env("STUB_LEAKS_STATUS", leaks).output().unwrap();
        assert_eq!(output.status.code(), Some(status), "{}\n{}", text(&output), fixture.trace());
        assert!(fixture.trace().contains("betterleaks "), "the link did not run the publication scan: {}", fixture.trace());
        assert_eq!(fixture.path("ran").exists(), status == 0, "{}", text(&output));
    }

    {
        let fixture = Fixture::new(&[("land.check", "file.txt :: ./bin/head-north land tests one"), ("land.check", "file.txt :: ./bin/head-north land tests two"), ("land.check", "file.txt :: true clippy"), ("land.check", "file.txt :: true nextest")]);
        let output = fixture.command().env("SAFE_PUSH_LAND_RUNNER", "buildbox").output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", text(&output));
        let trace = fixture.trace();
        assert!(trace.contains("capacity offload choose --any --count 2 --taken buildbox --taken buildbox\n"), "the 1:51 AM run of 2026-11-07 spread clippy and nextest onto ccx33 beside the head-north checks pinned there, counting none of them: {trace}");
    }

    {
        let fixture = Fixture::new(&[("land.runner", "build-host"), ("land.check", "file.txt :: ./bin/head-north land tests one"), ("land.check", "file.txt :: ./bin/head-north land tests two"), ("land.check", "file.txt :: true clippy"), ("land.check", "file.txt :: true nextest")]);
        let output = fixture.command().env("NORTH_LAND_CHECKS_OFF_HOST", "1").output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", text(&output));
        let trace = fixture.trace();
        assert!(trace.contains("capacity offload choose --any --avoid "), "the lander's candidate checks asked for a build host that may be this one: {trace}");
        assert!(trace.contains("--taken buildbox --taken buildbox --avoid "), "on 2026-11-08 the lander spread its candidate checks onto the laptop beside gex45 by free slots: {trace}");
    }

    {
        let fixture = Fixture::new(&[("land.runner", "buildbox"), ("land.check", "file.txt :: true clippy")]);
        let output = fixture.command().env("SAFE_PUSH_LAND_RUNNER", "").output().unwrap();
        assert_eq!(output.status.code(), Some(0), "{}", text(&output));
        let trace = fixture.trace();
        assert!(trace.contains("--host buildbox"), "on 2026-11-07 the lander unit's empty SAFE_PUSH_LAND_RUNNER overrode runner = build-host and every candidate check compiled on the laptop: {trace}");
    }
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

    fs::write(fixture.path("step.sh"), "if grep -q hang file.txt; then sleep 600; fi\n").unwrap();
    fs::remove_file(fixture.path("outcome")).unwrap();
    let timed = lane_onto(&fixture, "timed\n");
    let output = fixture.command_between(&fixture.head, &timed).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    fs::write(fixture.repo.join("file.txt"), "hang\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "hang"]);
    let lane = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&timed, &lane).env("SAFE_PUSH_LAND_CHECK_TIMEOUT", "2").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "on 2026-11-05 to 07 a land check past its p90-derived wall deadline on a loaded build host refused six lanes because the base tree had once finished it in time: {}", text(&output));
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(outcome.contains("skip\tland check bash ../step.sh ran past its 2 s deadline and base tree ") && !outcome.contains("failed-step"), "{outcome}");
}

impl Fixture {
}

const ENVIRONMENT_CHECK: &str = r#"printf '%s\n' "$(git rev-parse HEAD)" >>../runs; [ "$SAFE_PUSH_LAND_RUNTIME_HEAD" = "$(git rev-parse lane)" ] || exit 9; [ "$(git rev-parse "$SAFE_PUSH_LAND_HEAD")" = "$(git rev-parse HEAD)" ] || exit 8; [ ! -e ../plant ] || { echo "bash: nix: command not found" >&2; exit 127; }; if grep -q broken file.txt; then echo "error: the lane broke file.txt" >&2; exit 1; fi"#;

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
fn a_check_that_also_fails_on_base_on_an_overloaded_host_blames_the_load_not_main() {
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {ENVIRONMENT_CHECK}"))]);
    let lane = lane_onto(&fixture, "lane\n");
    fs::write(fixture.path("plant"), "").unwrap();
    fs::write(fixture.path("loadavg"), "500.00 500.00 500.00 1/100 1\n").unwrap();
    let output = fixture.command_between(&fixture.head, &lane).env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "{}", text(&output));
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(!outcome.contains("check environment is broken") && outcome.contains("while this host ran at load 500.0"), "on 2026-11-08 two timing tests failed on the lane and its base on the laptop at load 39 to 64, the lander read main as broken in placed and judged every candidate in place for 30 minutes, which drove the load to 123: {outcome}");
}

#[test]
fn a_test_that_is_red_on_the_base_never_refuses_the_lane_and_the_refusal_names_only_tests_the_base_passes() {
    let check = r#": cargo nextest run; if [ "$(cat file.txt)" = lane ]; then printf '        FAIL [   0.1s] (1/2) crate::bin red::on_both\n        FAIL [   0.1s] (2/2) crate::bin %s\n' "${LANE_ONLY:-lane::only}"; exit 100; fi; case "${SAFE_PUSH_LAND_IT:-}" in *red::on_both*) printf '        FAIL [   0.1s] (1/1) crate::bin red::on_both\n'; exit 100;; esac"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {check}"))]);
    let lane = lane_onto(&fixture, "lane\n");
    let output = fixture.command_between(&fixture.head, &lane).output().unwrap();
    let report = text(&output);
    assert_eq!(output.status.code(), Some(100), "a test the base passes still refuses the lane: {report}");
    assert!(report.contains("1 test(s) pass on base tree") && report.contains("fail on this lane: crate::bin lane::only") && report.contains("1 more fail on the base too"), "the refusal names the test the base passes and counts the one it fails: {report}");
    let red_only = fixture.command_between(&fixture.head, &lane).env("LANE_ONLY", "red::on_both").output().unwrap();
    assert_eq!(red_only.status.code(), Some(0), "every failing test is red on the base too, so the lane is not refused: {}", text(&red_only));
    assert!(text(&red_only).contains("the lane adds no failing test"), "{}", text(&red_only));
}

#[test]
fn a_check_that_reaches_no_verdict_names_no_failed_step_for_the_sheriff_and_is_not_rerun_on_base() {
    let dropped = r#"printf '%s\n' "$(git rev-parse HEAD)" >>../runs; echo "north capacity offload: the connection ended (ssh exit 255) before the remote host reported the command's exit, so the run reached no verdict" >&2; exit 75"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {dropped}"))]);
    let lane = lane_onto(&fixture, "lane\n");
    let output = fixture.command_between(&fixture.head, &lane).env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(75), "{}", text(&output));
    let outcome = fs::read_to_string(fixture.path("outcome")).unwrap();
    assert!(!outcome.lines().any(|line| line.starts_with("failed-step\t")), "a check with no verdict was named a failing step: {outcome}");
    assert!(outcome.lines().any(|line| line.starts_with("skip\tland check ") && line.ends_with("reached no verdict (exit 75: its host or connection failed before the check reported), which is not this lane's failure")), "{outcome}");
    assert_eq!(runs(&fixture), [lane], "a check with no verdict was rerun on the base tree");
}

#[test]
fn check_a_lane_adds_is_not_judged_against_base() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: printf x >>../runs; exit 127")]);
    let output = fixture.command().env("NORTH_LAND_ROUND", "r1").output().unwrap();
    assert_eq!(output.status.code(), Some(127), "{}", text(&output));
    assert_eq!(fs::read_to_string(fixture.path("runs")).unwrap(), "x", "a check the base tree does not list ran on it");
}

impl Fixture {
}


fn safe_push_entries(key: &str) -> Vec<String> {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(".safe-push");
    let output = Command::new("git").args(["config", "--file"]).arg(file).args(["--get-all", key]).output().unwrap();
    String::from_utf8(output.stdout).unwrap().lines().map(str::to_owned).collect()
}

fn split_entry(entry: &str) -> (&str, &str) {
    entry.split_once(" :: ").unwrap()
}

fn features_check() -> String {
    let entry = safe_push_entries("land.check").into_iter().find(|entry| entry.contains(" --no-build docs features-index")).expect("the features-index check runs before the push without a build");
    split_entry(&entry).1.to_owned()
}

const LEASE_CHECK: &str = "flock -n -x \"$(dirname \"$0\")/lease\" true 2>/dev/null && printf 'unleased %s\\n' \"$0\" >>\"$STUB_TRACE\"\n";

fn live_runtime(fixture: &Fixture) {
    let live = fixture.path("state/north/current/north");
    fs::create_dir_all(live.parent().unwrap()).unwrap();
    script(&live, &format!("if [ \"$1 $2\" = 'runtime covers' ]; then [ -z \"${{STUB_UNCOVERED:-}}\" ] || {{ echo 'runtime older than the checker code' >&2; exit 1; }}; printf '%s' \"$0\"; exit 0; fi\n{LEASE_CHECK}printf 'live %s\\n' \"$*\" >>\"$STUB_TRACE\"\nexec \"$REAL_NORTH\" \"$@\"\n"));
}

fn north_from_the_tree(fixture: &Fixture) {
    let tree = git(&fixture.repo, &["rev-parse", "HEAD^{tree}"]);
    let built = fixture.path(&format!("cache/north/head-north/{tree}/north"));
    fs::create_dir_all(built.parent().unwrap()).unwrap();
    script(&built, &format!("{LEASE_CHECK}printf 'tree %s\\n' \"$*\" >>\"$STUB_TRACE\"\nexec \"$REAL_NORTH\" \"$@\"\n"));
}

fn head_north(fixture: &Fixture, command: &str, uncovered: bool) -> Output {
    let mut run = Command::new("bash");
    run.current_dir(&fixture.repo).args(["-c", command]);
    run.env("REAL_NORTH", env!("CARGO_BIN_EXE_north")).env("STUB_TRACE", fixture.path("trace")).env("XDG_STATE_HOME", fixture.path("state")).env("XDG_CACHE_HOME", fixture.path("cache")).env("SAFE_PUSH_LAND_HEAD", git(&fixture.repo, &["rev-parse", "HEAD"]));
    run.env_remove("SAFE_PUSH_LAND_RUNTIME_HEAD");
    if uncovered {
        run.env("STUB_UNCOVERED", "1");
    }
    run.output().unwrap()
}

fn render_features(fixture: &Fixture, message: &str) -> String {
    let render = Command::new(env!("CARGO_BIN_EXE_north")).current_dir(&fixture.repo).args(["docs", "features-index"]).output().unwrap();
    assert!(render.status.success(), "{}", text(&render));
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", message]);
    git(&fixture.repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_features_entry_over_1200_characters_lands_and_its_check_runs_even_on_a_runtime_older_than_its_checker() {
    let command = features_check();
    let fixture = Fixture::new(&[("land.check-local", "true"), ("land.check", &format!("docs/features/ :: {command}"))]);
    live_runtime(&fixture);
    fs::create_dir_all(fixture.repo.join("bin")).unwrap();
    shim::copy_executable(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/head-north"), fixture.repo.join("bin/head-north"));
    let section = fixture.repo.join("docs/features/core");
    fs::create_dir_all(&section).unwrap();
    fs::write(fixture.repo.join("docs/features/_header.md"), "# Features\n").unwrap();
    fs::write(fixture.repo.join(".gitignore"), "/docs/features.md\n").unwrap();
    fs::write(section.join("_title.md"), "## Core\n").unwrap();
    fs::write(section.join("small.md"), "- A short entry.\n").unwrap();
    let clean = render_features(&fixture, "features");

    let output = head_north(&fixture, &command, false);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.trace().contains("live docs features-index"), "the covering live runtime did not run the checker: {}", fixture.trace());

    fs::write(section.join("long.md"), format!("- {}\n", "x".repeat(1_300))).unwrap();
    let over = render_features(&fixture, "a long entry");
    let output = head_north(&fixture, &command, true);
    assert_eq!(output.status.code(), Some(1), "a --no-build check on a runtime that does not cover its checker passed without running: {}", text(&output));
    assert!(text(&output).contains("head-north: refused, no live runtime covers the checker code"), "{}", text(&output));

    fs::write(fixture.path("trace"), "").unwrap();
    let landed = fixture.command_between(&clean, &over).output().unwrap();
    assert_eq!(landed.status.code(), Some(0), "the covering live runtime refused an entry only its body made long: {}", text(&landed));
    assert!(fixture.trace().contains("live docs features-index"), "{}", fixture.trace());

    north_from_the_tree(&fixture);
    fs::write(fixture.path("trace"), "").unwrap();
    let landed = fixture.command_between(&clean, &over).env("STUB_UNCOVERED", "1").output().unwrap();
    assert_eq!(landed.status.code(), Some(0), "a lane whose checker is newer than the live runtime failed its check before the push: {}", text(&landed));
    assert!(text(&landed).contains("the live runtime does not cover the checker code of 1 land check(s)"), "{}", text(&landed));
    let trace = fixture.trace();
    assert!(trace.contains("tree docs features-index") && !trace.contains("live docs features-index"), "the check did not run on north built from the lane's tree: {trace}");
    assert!(!trace.contains("unleased"), "a check ran on a runtime it held no lease on: {trace}");
}

fn head_north_on_a_runtime_mid_build() -> (Fixture, PathBuf) {
    let fixture = Fixture::new(&[("land.check-local", "true")]);
    live_runtime(&fixture);
    fs::create_dir_all(fixture.repo.join("bin")).unwrap();
    shim::copy_executable(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/head-north"), fixture.repo.join("bin/head-north"));
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "head-north"]);
    north_from_the_tree(&fixture);
    let lease = fixture.path("state/north/current/lease");
    fs::create_dir(&lease).unwrap();
    fs::write(fixture.path("trace"), "").unwrap();
    (fixture, lease)
}

fn run_head_north_releasing(fixture: &Fixture, command: &str, waiting_line: &str, release: impl FnOnce()) -> Output {
    let mut run = head_north_command(fixture, command);
    let mut child = run.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let mut seen = String::new();
    let mut release = Some(release);
    let mut line = String::new();
    while stderr.read_line(&mut line).unwrap() > 0 {
        if line.contains(waiting_line) {
            release.take().into_iter().for_each(|release| release());
        }
        seen.push_str(&line);
        line.clear();
    }
    let finished = child.wait_with_output().unwrap();
    Output { stderr: seen.into_bytes(), ..finished }
}

fn run_head_north(fixture: &Fixture, command: &str) -> Output {
    head_north_command(fixture, command).output().unwrap()
}

fn head_north_command(fixture: &Fixture, command: &str) -> Command {
    let mut run = Command::new("bash");
    run.current_dir(&fixture.repo).args(["-c", command]).env("HEAD_NORTH_SETTLE", "1");
    run.env("REAL_NORTH", env!("CARGO_BIN_EXE_north")).env("STUB_TRACE", fixture.path("trace")).env("XDG_STATE_HOME", fixture.path("state")).env("XDG_CACHE_HOME", fixture.path("cache")).env("SAFE_PUSH_LAND_HEAD", git(&fixture.repo, &["rev-parse", "HEAD"]));
    run
}

#[test]
fn head_north_refuses_a_tree_that_does_not_compile_and_reaches_no_verdict_only_when_the_build_itself_fails() {
    let fixture = Fixture::new(&[("land.check-local", "true")]);
    live_runtime(&fixture);
    fs::create_dir_all(fixture.repo.join("bin")).unwrap();
    shim::copy_executable(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/head-north"), fixture.repo.join("bin/head-north"));
    script(&fixture.repo.join("bin/land-shell"), "printf '%s\\n' \"$STUB_CARGO_ERROR\" >&2\nexit 101\n");
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "head-north"]);
    let build = |error: &str| head_north_command(&fixture, "./bin/head-north --build").env("STUB_UNCOVERED", "1").env("STUB_CARGO_ERROR", error).output().unwrap();
    let broken = build("error: could not compile `north-lander` (lib) due to 1 previous error");
    assert_eq!(broken.status.code(), Some(1), "a tree that does not compile read as no verdict: {}", text(&broken));
    assert!(text(&broken).contains("north does not compile from this tree"), "{}", text(&broken));
    let unfetched = build("error: failed to download `serde v1.0.0`");
    assert_eq!(unfetched.status.code(), Some(75), "a build that failed for want of the host's crates refused the lane: {}", text(&unfetched));
}

#[test]
fn a_land_check_waits_for_a_runtime_another_process_is_still_building_instead_of_failing() {
    let (fixture, lease) = head_north_on_a_runtime_mid_build();
    let output = run_head_north_releasing(&fixture, "./bin/head-north --code docs/ docs --help", "is not a whole runtime yet", || fs::remove_dir(lease).unwrap());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(text(&output).contains("is not a whole runtime yet"), "{}", text(&output));
    assert!(fixture.trace().contains("live docs --help"), "the check did not run on the runtime once it was whole: {}", fixture.trace());
}

#[test]
fn a_land_check_runs_on_the_trees_own_north_when_the_live_runtime_stays_unreadable() {
    let (fixture, _lease) = head_north_on_a_runtime_mid_build();
    let output = run_head_north(&fixture, "./bin/head-north --code docs/ docs --help");
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.trace().contains("tree docs --help") && !fixture.trace().contains("live docs --help"), "{}", fixture.trace());
    let refused = run_head_north(&fixture, "./bin/head-north --code docs/ --no-build docs --help");
    assert_eq!(refused.status.code(), Some(1), "{}", text(&refused));
    assert!(text(&refused).contains("stayed unreadable"), "{}", text(&refused));
}

const HEAD_NORTH_SHIM: &str = "[ \"$1\" = --code ] && shift 2\n[ \"$1\" = --build ] && exit 0\n[ \"$1\" = --no-build ] && shift\nexec \"$REAL_NORTH\" \"$@\"\n";

fn with_north_land_lines(repo: &Path) {
    fs::write(repo.join(".safe-push"), include_str!("../../.safe-push")).unwrap();
    git(repo, &["config", "--file", ".safe-push", "--remove-section", "landing"]);
    git(repo, &["config", "--file", ".safe-push", "--unset", "land.runner"]);
    fs::create_dir_all(repo.join("bin")).unwrap();
    script(&repo.join("bin/head-north"), HEAD_NORTH_SHIM);
    script(&repo.join("bin/land-shell"), "exit 0\n");
}

#[test]
fn north_land_check_refuses_a_lane_that_grows_a_skill_past_its_byte_limit() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    let repo = &fixture.repo;
    with_north_land_lines(repo);
    let skill = repo.join("harness/skills/demo/SKILL.md");
    fs::create_dir_all(skill.parent().unwrap()).unwrap();
    fs::write(&skill, "---\nname: demo\ndescription: Demo skill.\n---\n\n# Demo\n\nRoute to references.\n").unwrap();
    git(repo, &["add", ".safe-push", "harness", "bin"]);
    git(repo, &["commit", "-qm", "skill"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    let mut grown = fs::read_to_string(&skill).unwrap();
    grown.push_str(&"- Another rule the skill body grew by.\n".repeat(170));
    let bytes = grown.len();
    fs::write(&skill, grown).unwrap();
    git(repo, &["commit", "-qam", "grow the skill"]);
    let head = git(repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&base, &head).output().unwrap();
    let report = text(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert!(report.contains(&format!("demo: SKILL.md is {bytes} bytes; policy keeps each SKILL.md at most 6144")), "{report}");
}

#[test]
fn north_land_check_refuses_a_lane_that_adds_a_reference_its_module_layers_forbid() {
    let fixture = Fixture::new(&[("land.check", "file.txt :: true")]);
    let repo = &fixture.repo;
    with_north_land_lines(repo);
    fs::write(repo.join("structure.toml"), "[[module]]\nname = \"tests\"\npaths = [\"src/store/tests.rs\"]\nmay_use = []\n\n[[module]]\nname = \"fleet_store\"\npaths = [\"src/store/mod.rs\"]\nmay_use = []\n").unwrap();
    fs::write(repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    fs::create_dir_all(repo.join("src/store")).unwrap();
    fs::write(repo.join("src/main.rs"), "mod store;\n").unwrap();
    fs::write(repo.join("src/store/mod.rs"), "pub fn open() {}\n").unwrap();
    fs::write(repo.join("src/store/tests.rs"), "pub fn check() {}\n").unwrap();
    git(repo, &["add", ".safe-push", "bin", "structure.toml", "Cargo.toml", "src"]);
    git(repo, &["commit", "-qm", "store"]);
    let base = git(repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("src/store/tests.rs"), "pub fn check() {\n    crate::store::open();\n}\n").unwrap();
    git(repo, &["commit", "-qam", "split the store"]);
    let head = git(repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&base, &head).output().unwrap();
    let report = text(&output);
    assert_eq!(output.status.code(), Some(1), "{report}");
    assert!(report.contains("module tests may not use fleet_store"), "{report}");
}

#[test]
fn land_after_reuses_the_candidates_selection() {
    let record = r#"printf '%s\n' "$SAFE_PUSH_LAND_IT" >>../seen"#;
    let line = format!("crates/ :: {record}");
    let fixture = Fixture::new(&[("land.check", &line), ("land.after", &line)]);
    fs::write(fixture.repo.join("structure.toml"), "[[crate_dag.crate]]\nname = \"a\"\npaths = [\"crates/a/src/**\"]\nmay_use = []\n\n[[crate_dag.crate]]\nname = \"b\"\npaths = [\"crates/b/src/**\"]\nmay_use = []\n").unwrap();
    fs::create_dir_all(fixture.repo.join("tests/it")).unwrap();
    fs::write(fixture.repo.join("tests/it/crates.txt"), "m1 a\nm2 b\n").unwrap();
    fs::write(fixture.repo.join("tests/it/tree_wide_units.txt"), "# no tree-wide unit tests in this fixture\n").unwrap();
    git(&fixture.repo, &["add", "structure.toml", "tests"]);
    git(&fixture.repo, &["commit", "-qm", "structure"]);
    let before = git(&fixture.repo, &["rev-parse", "HEAD"]);
    fs::create_dir_all(fixture.repo.join("crates/a/src")).unwrap();
    fs::write(fixture.repo.join("crates/a/src/f.rs"), "\n").unwrap();
    git(&fixture.repo, &["add", "crates"]);
    git(&fixture.repo, &["commit", "-qm", "crate a"]);
    let tip = git(&fixture.repo, &["rev-parse", "HEAD"]);

    let candidate = fixture.command_between(&before, &tip).output().unwrap();
    assert_eq!(candidate.status.code(), Some(0), "{}", text(&candidate));
    let after = fixture.command_between(&tip, &tip).args(["--after", "--command", record]).output().unwrap();
    assert_eq!(after.status.code(), Some(0), "{}", text(&after));
    let seen = fs::read_to_string(fixture.path("seen")).unwrap();
    let runs: Vec<&str> = seen.lines().collect();
    assert_eq!(runs.len(), 2, "{seen}\n{}\n{}", text(&candidate), text(&after));
    assert!(runs[0].contains("test(/m1::/)") && !runs[0].contains("m2::"), "the candidate's selection should name only the module of the changed crate: {seen}");
    assert_eq!(runs[0], runs[1], "the after-landing run selected differently from its candidate: {seen}");
    let fresh = fixture.command_between(&tip, &tip).args(["--after", "--fresh", "--command", record]).output().unwrap();
    assert_eq!(fresh.status.code(), Some(0), "the sheriff's --fresh rerun is a flag of the after-landing runner: {}", text(&fresh));
    assert_eq!(fs::read_to_string(fixture.path("seen")).unwrap().lines().count(), 3, "a --fresh after-landing run runs its check: {}", text(&fresh));
}
