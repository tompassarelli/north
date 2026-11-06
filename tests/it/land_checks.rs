use north_core::foundation::shim;
use std::fs;
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
        let output = fixture.command_through(&fixture.base, &fixture.head, &link).env("SAFE_PUSH_DENYLIST", fixture.path("absent-denylist.toml")).env("SAFE_PUSH_SOUTH", fixture.path("absent-south")).env("STUB_LEAKS_STATUS", leaks).output().unwrap();
        assert_eq!(output.status.code(), Some(status), "{}\n{}", text(&output), fixture.trace());
        assert!(fixture.trace().contains("betterleaks "), "the link did not run the publication scan: {}", fixture.trace());
        assert_eq!(fixture.path("ran").exists(), status == 0, "{}", text(&output));
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

fn verb(command: &str) -> Option<&str> {
    let rest = command.split_once(" --code ")?.1.split_once(' ')?.1;
    Some(rest.strip_prefix("--no-build ").unwrap_or(rest))
}

fn builds_on_the_remote(entry: &str) -> bool {
    entry.split_once(" --remote-north \"").is_some_and(|(_, remote)| remote.split('"').next().is_some_and(|north| !north.contains(" --no-build")))
}

fn before_the_push() -> Vec<String> {
    safe_push_entries("land.check").into_iter().filter(|entry| entry.contains(" --no-build ") && entry.contains("./bin/head-north --code ") && !builds_on_the_remote(entry)).collect()
}

#[test]
fn every_check_run_before_the_push_without_a_build_has_an_after_landing_backstop() {
    let after = safe_push_entries("land.after");
    let early = before_the_push();
    assert_eq!(early.len(), 11, "{early:?}");
    for entry in early {
        let (patterns, command) = split_entry(&entry);
        let selected_tests = verb(command).is_some_and(|verb| verb.starts_with("coverage test "));
        let backstop = after.iter().find(|candidate| if selected_tests { split_entry(candidate).1.contains("cargo nextest run") } else { verb(split_entry(candidate).1) == verb(command) });
        let (after_patterns, _) = split_entry(backstop.unwrap_or_else(|| panic!("no land.after line builds and runs {command}")));
        assert_eq!(after_patterns, patterns, "a lane the check skips for want of a covering runtime must still reach its after-landing run: {command}");
    }
}

const LIVE_RUNTIME: &str = "if [ \"$1 $2\" = 'runtime covers' ]; then [ -z \"${STUB_UNCOVERED:-}\" ] || { echo 'runtime older than the checker code' >&2; exit 1; }; printf '%s' \"$0\"; exit 0; fi\nprintf '%s\\n' \"$*\" >>\"$STUB_TRACE\"\nexec \"$REAL_NORTH\" \"$@\"\n";

fn live_runtime(fixture: &Fixture) {
    let live = fixture.path("state/north/current/north");
    fs::create_dir_all(live.parent().unwrap()).unwrap();
    script(&live, LIVE_RUNTIME);
}

fn with_real_head_north(command: &str) -> String {
    command.replace("./bin/head-north", Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/head-north").to_str().unwrap())
}

fn head_north(fixture: &Fixture, command: &str, uncovered: bool) -> Command {
    let mut run = Command::new("bash");
    run.current_dir(&fixture.repo).args(["-c", command]);
    run.env("REAL_NORTH", env!("CARGO_BIN_EXE_north")).env("STUB_TRACE", fixture.path("live-trace")).env("XDG_STATE_HOME", fixture.path("state")).env("SAFE_PUSH_LAND_HEAD", &fixture.head);
    if uncovered {
        run.env("STUB_UNCOVERED", "1");
    }
    run
}

#[test]
fn a_features_entry_over_its_cap_is_refused_before_the_push() {
    let entry = before_the_push().into_iter().find(|entry| entry.contains("docs features-index --check")).expect("the features-index check runs before the push");
    let command = with_real_head_north(split_entry(&entry).1);
    let command = command.as_str();
    let fixture = Fixture::new(&[("land.check", &format!("docs/features/ :: {command}"))]);
    live_runtime(&fixture);
    let section = fixture.repo.join("docs/features/core");
    fs::create_dir_all(&section).unwrap();
    fs::write(fixture.repo.join("docs/features/_header.md"), "# Features\n").unwrap();
    fs::write(section.join("_title.md"), "## Core\n").unwrap();
    fs::write(section.join("small.md"), "- A short entry.\n").unwrap();
    let render = Command::new(env!("CARGO_BIN_EXE_north")).current_dir(&fixture.repo).args(["docs", "features-index"]).output().unwrap();
    assert!(render.status.success(), "{}", text(&render));
    git(&fixture.repo, &["add", "docs"]);
    git(&fixture.repo, &["commit", "-qm", "features"]);
    let clean = git(&fixture.repo, &["rev-parse", "HEAD"]);

    let run_check = |uncovered: bool| head_north(&fixture, command, uncovered).output().unwrap();
    let output = run_check(false);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(fixture.path("live-trace").exists(), "the covering live runtime did not run the checker");

    fs::write(section.join("long.md"), format!("- {}\n", "x".repeat(1_300))).unwrap();
    let render = Command::new(env!("CARGO_BIN_EXE_north")).current_dir(&fixture.repo).args(["docs", "features-index"]).output().unwrap();
    assert!(render.status.success(), "{}", text(&render));
    let output = run_check(false);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains("over the 1200-character entry cap"), "{}", text(&output));

    fs::remove_file(fixture.path("live-trace")).unwrap();
    let output = run_check(true);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(text(&output).contains("not run before the push"), "{}", text(&output));
    assert!(!fixture.path("live-trace").exists(), "a runtime that does not cover the checker ran it anyway");

    git(&fixture.repo, &["add", "docs"]);
    git(&fixture.repo, &["commit", "-qm", "over the cap"]);
    let over = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let landed = fixture.command_between(&clean, &over).output().unwrap();
    assert_eq!(landed.status.code(), Some(1), "the lander ran the check from the lane's own .safe-push and the cap refused the lane: {}", text(&landed));
    assert!(text(&landed).contains("over the 1200-character entry cap"), "{}", text(&landed));
}

const REUSED_CHECK: &str = "printf 'ran %s\\n' \"$*\" >>../check-runs\n[ ! -e ../defer ] || { echo 'head-north: not run before the push, no live runtime covers the checker code' >&2; exit 0; }\ngrep -q changed file.txt\n";

fn reuse_fixture() -> Fixture {
    let fixture = Fixture::new(&[("land.check", "file.txt :: bash check.sh --no-build verify"), ("land.after", "file.txt :: bash check.sh verify")]);
    fs::write(fixture.repo.join("check.sh"), REUSED_CHECK).unwrap();
    git(&fixture.repo, &["add", "check.sh"]);
    git(&fixture.repo, &["commit", "-qm", "check"]);
    Fixture { head: git(&fixture.repo, &["rev-parse", "HEAD"]), ..fixture }
}

fn checked(fixture: &Fixture, base: &str, head: &str, after: bool) -> Output {
    let mut command = fixture.command_between(base, head);
    if after {
        command.args(["--after", "--dry", "1"]);
    }
    command.env("STUB_VERDICTS", fixture.path("verdicts")).env("NORTH_LAND_SPANS", fixture.path("spans.jsonl")).output().unwrap()
}

fn check_runs(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(fixture.path("check-runs")).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn reused_spans(fixture: &Fixture) -> Vec<String> {
    fs::read_to_string(fixture.path("spans.jsonl")).unwrap_or_default().lines().filter(|line| line.contains("reused verdict ")).map(str::to_owned).collect()
}

#[test]
fn an_after_landing_check_whose_inputs_passed_before_the_push_reuses_that_verdict_and_reruns_once_an_input_changes() {
    let fixture = reuse_fixture();
    let pushed = checked(&fixture, &fixture.base, &fixture.head, false);
    assert_eq!(pushed.status.code(), Some(0), "{}", text(&pushed));
    assert_eq!(check_runs(&fixture), ["ran --no-build verify"]);

    git(&fixture.repo, &["commit", "-q", "--allow-empty", "-m", "the lane landed as a new commit with the tree it was checked on"]);
    let landed = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let after = checked(&fixture, &fixture.base, &landed, true);
    assert_eq!(after.status.code(), Some(0), "{}", text(&after));
    assert_eq!(check_runs(&fixture), ["ran --no-build verify"], "the after-landing run re-ran a check whose inputs the pre-push run judged: {}", text(&after));
    let spans = reused_spans(&fixture);
    assert_eq!(spans.len(), 1, "{spans:?}");
    assert!(spans[0].contains("bash check.sh verify") && spans[0].contains("\"cache\":\"hit\""), "{spans:?}");
    assert!(text(&after).contains("land check: bash check.sh verify: reused verdict "), "{}", text(&after));

    fs::write(fixture.repo.join("file.txt"), "changed again\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "an input the check reads"]);
    let moved = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let rerun = checked(&fixture, &landed, &moved, true);
    assert_eq!(rerun.status.code(), Some(0), "{}", text(&rerun));
    assert_eq!(check_runs(&fixture), ["ran --no-build verify", "ran verify"], "a changed input did not re-run the after-landing check: {}", text(&rerun));
    assert_eq!(reused_spans(&fixture).len(), 1);
    a_pre_push_check_that_head_north_deferred_leaves_its_after_landing_line_to_run();
}

fn a_pre_push_check_that_head_north_deferred_leaves_its_after_landing_line_to_run() {
    let fixture = reuse_fixture();
    fs::write(fixture.path("defer"), "").unwrap();
    let pushed = checked(&fixture, &fixture.base, &fixture.head, false);
    assert_eq!(pushed.status.code(), Some(0), "{}", text(&pushed));
    fs::remove_file(fixture.path("defer")).unwrap();
    let after = checked(&fixture, &fixture.base, &fixture.head, true);
    assert_eq!(after.status.code(), Some(0), "{}", text(&after));
    assert_eq!(check_runs(&fixture), ["ran --no-build verify", "ran verify"], "a deferred pre-push pass stood in for the after-landing check: {}", text(&after));
    assert!(reused_spans(&fixture).is_empty());
}
