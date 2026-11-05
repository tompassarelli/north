use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const FAKE_GH: &str = r#"state=$FAKE_GH_STATE
case "$1 $2" in
  "workflow run")
    shift 3
    while [ $# -gt 0 ]; do
      case "$1" in
        --ref) ref=$2 ;;
        -f) case "$2" in head=*) head=${2#head=} ;; nonce=*) nonce=${2#nonce=} ;; commands=*) printf '%s\n' "${2#commands=}" >"$state/commands" ;; esac ;;
      esac
      shift 2
    done
    git ls-remote --exit-code "$FAKE_ORIGIN" "refs/heads/$ref" >/dev/null || exit 1
    printf 'Land check %s %s' "$head" "$nonce" >"$state/title"
    ;;
  "run list") printf '[{"databaseId":7,"displayTitle":"%s","url":"https://github.invalid/runs/7","status":"completed","conclusion":"%s"}]\n' "$(cat "$state/title")" "$(cat "$state/conclusion" 2>/dev/null || echo "$FAKE_CONCLUSION")" ;;
  "run view") case "$*" in *--log-failed*) printf '%s\n' "${FAKE_FAILED_LOG:-land check exited 23}" ;; *) printf '{"jobs":[{"name":"remote check","conclusion":"%s"}]}\n' "$FAKE_CONCLUSION" ;; esac ;;
  "issue list") echo '[]' ;;
  "issue create") echo 'https://github.invalid/issues/812' ;;
  "repo view") echo PUBLIC ;;
  *) exit 1 ;;
esac
"#;

const FAKE_NORTH: &str = r#"state=$FAKE_GH_STATE
case "$1 $2" in
  "capacity offload")
    printf '%s\n' "$*" >>"$state/offload"
    if [ "$FAKE_MAIN_RED" = 1 ]; then echo '---- shared_test stdout ----'; exit 1; fi
    echo 'test shared_test ... ok'
    exit 0
    ;;
  "fleet list") echo '[{"name":"worker-1","parent":"lead-1"}]'; exit 0 ;;
  "fleet send")
    printf '%s\t%s\n' "$5" "$6" >>"$state/events"
    if [ ! -e "$state/conclusion" ]; then
      git -C "$FAKE_FIXER" pull -q origin main
      git -C "$FAKE_FIXER" commit -q --allow-empty -m 'Fix shared_test' -m 'Main-red: #812'
      git -C "$FAKE_FIXER" push -q origin HEAD:main
      echo success >"$state/conclusion"
    fi
    exit 0
    ;;
esac
exec "$REAL_NORTH" "$@"
"#;

const REMOTE_CHECK: &str = "exit 23";
const LOCAL_CHECK: &str = "echo ran >>\"$CHECK_LOG\"";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn executable(path: &Path, body: &str) {
    shim::write_script(path, body);
}

struct Landing {
    _fixture: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    origin: PathBuf,
}

impl Landing {
    fn new() -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().to_owned();
        let (repo, origin, tools) = (root.join("home/code/proj/main"), root.join("origin.git"), root.join("tools"));
        for directory in [&repo, &tools, &root.join("gh"), &root.join("home/.local/state/north/current")] {
            fs::create_dir_all(directory).unwrap();
        }
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Land actions scenario"]);
        git(&repo, &["config", "user.email", "actions@example.invalid"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        let config = |key: &str, value: &str| git(&repo, &["config", "--file", ".safe-push", "--add", key, value]);
        config("land.workflow", "land-check.yml");
        config("land.check", &format!("** :: {REMOTE_CHECK}"));
        config("land.check", &format!("** :: {LOCAL_CHECK}"));
        config("land.local-only", &format!("reads this host's checkouts :: {LOCAL_CHECK}"));
        fs::write(repo.join("file"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Land checks on Actions"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        executable(&tools.join("gh"), FAKE_GH);
        executable(&tools.join("gitleaks"), "exit 0\n");
        executable(&tools.join("north"), FAKE_NORTH);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), "fixer"]);
        git(&root.join("fixer"), &["config", "user.name", "Fixer"]);
        git(&root.join("fixer"), &["config", "user.email", "fixer@example.invalid"]);
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("home/.local/state/north/current/north")).unwrap();
        Landing { _fixture: fixture, root, repo, origin }
    }

    fn lane_commit(&self) -> String {
        fs::write(self.repo.join("file"), "lane\n").unwrap();
        git(&self.repo, &["commit", "-qam", "Lane change"]);
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn land(&self, conclusion: &str) -> Output {
        self.land_with(conclusion, &[])
    }

    fn land_with(&self, conclusion: &str, extra: &[(&str, &str)]) -> Output {
        let mut paths = vec![self.root.join("tools")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/safe-push"))
            .current_dir(&self.repo)
            .args(["--to", "main"])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("FAKE_GH_STATE", self.root.join("gh"))
            .env("FAKE_ORIGIN", &self.origin)
            .env("FAKE_CONCLUSION", conclusion)
            .env("CHECK_LOG", self.root.join("check.log"))
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("FAKE_FIXER", self.root.join("fixer"))
            .env("NORTH_FLEET_SESSION", "worker-1")
            .env("NORTH_RED_WAIT_POLL", "1")
            .envs(extra.iter().copied())
            .env_remove("GITHUB_ACTIONS")
            .env_remove("SAFE_PUSH_TEST_REAL_NORTH")
            .output()
            .unwrap()
    }

    fn landed(&self) -> String {
        git(&self.origin, &["rev-parse", "main"])
    }

    fn dispatched(&self) -> String {
        fs::read_to_string(self.root.join("gh/commands")).unwrap_or_default()
    }

    fn state(&self, name: &str) -> String {
        fs::read_to_string(self.root.join("gh").join(name)).unwrap_or_default()
    }

    fn local_runs(&self) -> usize {
        fs::read_to_string(self.root.join("check.log")).unwrap_or_default().lines().count()
    }
}

fn text(output: &Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

#[test]
fn a_failed_actions_run_refuses_the_landing_and_removes_its_scratch_branch() {
    let landing = Landing::new();
    let before = landing.landed();
    landing.lane_commit();

    let output = landing.land("failure");

    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert_eq!(landing.landed(), before, "a red Actions verdict published the lane");
    assert!(text(&output).contains("concluded failure; failed: remote check (failure)"), "{}", text(&output));
    assert!(text(&output).contains("land check exited 23"), "the failed log reaches the worker: {}", text(&output));
    assert_eq!(landing.dispatched().trim(), format!("[\"{REMOTE_CHECK}\"]"), "only the portable check goes to Actions");
    assert!(text(&output).contains("runs on this host because it reads this host's checkouts"), "{}", text(&output));
    assert_eq!(landing.local_runs(), 1, "the local-only check ran here");
    assert_eq!(git(&landing.origin, &["for-each-ref", "refs/heads/land-check"]), "", "the scratch branch is deleted");
}

const SHARED_FAILURE: &str = "remote check\tLand check\t2026-11-04T06:33:01.1234567Z ---- shared_test stdout ----";

#[test]
fn a_test_that_also_fails_on_mains_own_tree_raises_the_red_main_event_and_retries_after_the_fix() {
    let landing = Landing::new();
    let lane = landing.lane_commit();

    let output = landing.land_with("failure", &[("FAKE_FAILED_LOG", SHARED_FAILURE), ("FAKE_MAIN_RED", "1")]);

    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    assert!(text(&output).contains("blocked by red main, not this lane"), "{}", text(&output));
    assert!(landing.state("offload").contains("--exact 'shared_test'"), "only the failing test reruns on main: {}", landing.state("offload"));
    let events = landing.state("events");
    let targets: Vec<&str> = events.lines().filter_map(|line| line.split('\t').next()).collect();
    assert_eq!(targets, ["proxy", "lead-1"], "{events}");
    assert!(events.contains("main is red at") && events.contains("shared_test fails on main's own tree") && events.contains("Main-red: #812"), "{events}");
    assert!(git(&landing.origin, &["log", "--format=%s", "main"]).starts_with("Lane change\nFix shared_test"), "the lane landed after the fix");
    assert_ne!(landing.landed(), lane);
}

#[test]
fn a_test_that_fails_only_on_the_lane_refuses_the_landing_without_an_event() {
    let landing = Landing::new();
    let before = landing.landed();
    landing.lane_commit();

    let output = landing.land_with("failure", &[("FAKE_FAILED_LOG", SHARED_FAILURE), ("FAKE_MAIN_RED", "0")]);

    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert_eq!(landing.landed(), before);
    assert!(text(&output).contains("pass on main's own tree"), "{}", text(&output));
    assert!(!text(&output).contains("blocked by red main"), "{}", text(&output));
    assert_eq!(landing.state("events"), "", "no red-main event for a lane's own failure");
}
