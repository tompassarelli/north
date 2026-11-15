use north_foundation::foundation::shim;
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
        git(&repo, &["update-ref", "refs/remotes/origin/main", &base]);
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
        Self { root, repo, head }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn command_between(&self, base: &str, head: &str) -> Command {
        self.command_through(base, head, &self.path("stubs/safe-push"))
    }

    fn command_through(&self, base: &str, head: &str, safe_push: &Path) -> Command {
        let mut command = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"));
        let path = format!("{}:{}", self.path("stubs").display(), std::env::var("PATH").unwrap_or_default());
        command
            .current_dir(&self.repo)
            .args(["land", "checks", "--base", base, "--head", head, "--safe-push"])
            .arg(safe_push)
            .arg("--outcome")
            .arg(self.path("outcome"))
            .env("PATH", path)
            .env("NORTH_LAND_NORTH", self.path("stubs/north"))
            .env("DEPBOX", "1")
            .env("REAL_NORTH", crate::support::bin_exe!("CARGO_BIN_EXE_north"))
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

}

fn text(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
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
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {ENVIRONMENT_CHECK}")), ("land.reports-all", &format!("it names the broken file, its only finding :: {ENVIRONMENT_CHECK}"))]);
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
    assert_eq!(output.status.code(), Some(127), "outside a lander round a failure of the check's environment stands, though the base fails it too: {}", text(&output));
    assert_eq!(runs(&fixture).len(), 7, "before queueing the base tree ran the failed check once to tell whose failure it is");

    fs::remove_file(fixture.path("plant")).unwrap();
    fs::remove_file(fixture.path("outcome")).unwrap();
    let broken = lane_onto(&fixture, "broken\n");
    let output = fixture.command_between(&fixture.head, &broken).env("NORTH_LAND_ROUND", "r3").output().unwrap();
    assert_eq!(output.status.code(), Some(1), "a check the base tree passes refuses the lane: {}", text(&output));
    assert!(!fs::read_to_string(fixture.path("outcome")).unwrap().contains("environment is broken"));
    assert_eq!(runs(&fixture)[7..], [broken.clone(), fixture.head.clone()]);
    assert_eq!(git(&fixture.repo, &["rev-parse", "HEAD"]), broken);

    git(&fixture.repo, &["checkout", "-q", "-B", "lane", &broken]);
    fs::write(fixture.repo.join("file.txt"), "broken\nand a lane's own line\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "a lane on a red main"]);
    let innocent = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&broken, &innocent).output().unwrap();
    assert_eq!(output.status.code(), Some(0), "on 2026-11-15 five lanes were refused before queueing by checks main already failed the same way: {}", text(&output));
    let output = fixture.command_between(&broken, &innocent).env("NORTH_LAND_ROUND", "r4").output().unwrap();
    assert_eq!(output.status.code(), Some(0), "a check main fails the same way paused the lander for every lane: {}", text(&output));
    assert!(fs::read_to_string(fixture.path("outcome")).unwrap().contains("not gating"));
}

#[test]
fn a_test_that_is_red_on_the_base_never_refuses_the_lane_and_the_refusal_names_only_tests_the_base_passes() {
    let check = r#": cargo nextest run; if [ "$(cat file.txt)" = lane ]; then printf '        FAIL [   0.1s] (1/2) crate::bin red::on_both\n        FAIL [   0.1s] (2/2) crate::bin %s\n     Summary [   0.2s] 2 tests run: 0 passed, 2 failed, 0 skipped\n' "${LANE_ONLY:-lane::only}"; exit 100; fi; case "${SAFE_PUSH_LAND_IT:-}" in *red::on_both*) printf '        FAIL [   0.1s] (1/1) crate::bin red::on_both\n'; exit 100;; esac"#;
    let fixture = Fixture::new(&[("land.check", &format!("file.txt :: {check}"))]);
    git(&fixture.repo, &["checkout", "-q", "-B", "tested", &fixture.head]);
    fs::create_dir_all(fixture.repo.join("src")).unwrap();
    fs::write(fixture.repo.join("Cargo.toml"), "[package]\nname = \"fixture\"\n").unwrap();
    fs::write(fixture.repo.join("src/lib.rs"), "#[test]\nfn on_both() {}\n").unwrap();
    git(&fixture.repo, &["add", "Cargo.toml", "src/lib.rs"]);
    git(&fixture.repo, &["commit", "-qm", "the base defines its red test"]);
    let tested = git(&fixture.repo, &["rev-parse", "HEAD"]);
    git(&fixture.repo, &["checkout", "-q", "-B", "lane", &tested]);
    fs::write(fixture.repo.join("file.txt"), "lane\n").unwrap();
    git(&fixture.repo, &["commit", "-qam", "lane"]);
    let lane = git(&fixture.repo, &["rev-parse", "HEAD"]);
    let output = fixture.command_between(&tested, &lane).output().unwrap();
    let report = text(&output);
    assert_eq!(output.status.code(), Some(100), "a test the base passes still refuses the lane: {report}");
    assert!(report.contains("1 test(s) pass on base tree") && report.contains("fail on this lane: crate::bin lane::only") && report.contains("1 more fail on the base too"), "the refusal names the test the base passes and counts the one it fails: {report}");
    let red_only = fixture.command_between(&tested, &lane).env("LANE_ONLY", "red::on_both").output().unwrap();
    assert_eq!(red_only.status.code(), Some(0), "every failing test is red on the base too, so the lane is not refused: {}", text(&red_only));
    assert!(text(&red_only).contains("the lane adds no failing test"), "{}", text(&red_only));
}

impl Fixture {
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

