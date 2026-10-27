use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOM: &str = "20\n1.50 1.00 1.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 %KIB% 90% /\n";
const GIB: u64 = 1024 * 1024;

const FAKE_SSH: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do
  case "$1" in
    -o) shift 2 ;;
    -T) shift ;;
    --) shift; break ;;
    *) destination=${1#*@}; shift ;;
  esac
done
printf '%s\n' "$destination" >>"$FAKE_SSH_LOG"
case "$*" in
  *nproc*) [ -f "$FAKE_ROOMS/$destination" ] || exit 255; cat "$FAKE_ROOMS/$destination"; exit 0 ;;
esac
[ -d "$FAKE_REMOTE_HOMES/$destination" ] || exit 255
HOME=$FAKE_REMOTE_HOMES/$destination exec sh -c "$*"
"#;

const CAPACITY_STUB: &str = "#!/bin/sh\nprintf '%s\\n' \"$1\" >>\"$CAPACITY_LOG\"\nshift\nexec \"$@\"\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect()
}

struct Landing {
    _fixture: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    origin: PathBuf,
    remote: PathBuf,
}

impl Landing {
    fn new(check: &str, local_only: bool) -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().to_owned();
        let (repo, origin, remote) = (root.join("home/code/proj/main"), root.join("origin.git"), root.join("remotes/gex45/code/proj/main"));
        for directory in [&repo, &root.join("tools"), &root.join("rooms"), &root.join("run/agent-capacity-v1"), &root.join("home/.local/state/north/current")] {
            fs::create_dir_all(directory).unwrap();
        }
        git(&root, &["init", "-q", "--bare", "-b", "main", "origin.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Land offload scenario"]);
        git(&repo, &["config", "user.email", "offload@example.invalid"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        let tools = root.join("tools");
        let stub = |class: &str| format!("{} {class} --", tools.join("capacity-stub").display());
        let config = |key: &str, value: &str| git(&repo, &["config", "--file", ".safe-push", "--add", key, value]);
        config("landing.capacity", &stub("held"));
        config("landing.offloaded-capacity", &stub("offloaded"));
        config("land.check", &format!("** :: {check}"));
        if local_only {
            config("land.local-only", check);
        }
        fs::write(repo.join("verdict"), "ok\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Land checks"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), remote.to_str().unwrap()]);
        git(&repo, &["switch", "-qc", "lane"]);
        executable(&tools.join("ssh"), FAKE_SSH);
        executable(&tools.join("capacity-stub"), CAPACITY_STUB);
        executable(&tools.join("gitleaks"), "#!/bin/sh\nexit 0\n");
        executable(&tools.join("gh"), "#!/bin/sh\ncase \"$1\" in repo) echo PUBLIC;; run) echo '[]';; *) exit 1;; esac\n");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), tools.join("north")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("home/.local/state/north/current/north")).unwrap();
        fs::write(
            root.join("orchestration.toml"),
            "[placement]\norder = [\"whiterabbit\", \"gex45\", \"nexus\"]\n[hosts.whiterabbit]\nssh = \"tom@whiterabbit\"\n[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.nexus]\nssh = \"tom@nexus\"\n",
        )
        .unwrap();
        Landing { _fixture: fixture, root, repo, origin, remote }
    }

    fn gaming(&self, gaming: bool) {
        let at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        fs::write(self.root.join("run/agent-capacity-v1/gaming.json"), format!("{{\"gaming\":{gaming},\"at\":{at}}}")).unwrap();
    }

    fn room(&self, host: &str, disk_gib: u64) {
        fs::write(self.root.join("rooms").join(host), ROOM.replace("%KIB%", &(disk_gib * GIB).to_string())).unwrap();
    }

    fn lane_commit(&self, verdict: &str) -> String {
        fs::write(self.repo.join("verdict"), format!("{verdict}\n")).unwrap();
        let count = lines(&self.root.join("commits")).len();
        fs::write(self.repo.join(format!("change-{count}")), "lane change\n").unwrap();
        fs::write(self.root.join("commits"), "x\n".repeat(count + 1)).unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", &format!("Lane says {verdict} {count}")]);
        git(&self.repo, &["rev-parse", "HEAD"])
    }

    fn land(&self) -> Output {
        let mut paths = vec![self.root.join("tools")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/bin/safe-push"))
            .current_dir(&self.repo)
            .args(["--to", "main"])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_RUNTIME_DIR", self.root.join("run"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("RESIDENT_CAP_HOST", "whiterabbit")
            .env("GAME_RESERVE_STATE", self.root.join("no-game-reserve.json"))
            .env("FAKE_SSH_LOG", self.root.join("ssh.log"))
            .env("FAKE_ROOMS", self.root.join("rooms"))
            .env("FAKE_REMOTE_HOMES", self.root.join("remotes"))
            .env("CAPACITY_LOG", self.root.join("capacity.log"))
            .env("CHECK_LOG", self.root.join("check.log"))
            .env_remove("SAFE_PUSH_TEST_REAL_NORTH")
            .output()
            .unwrap()
    }

    fn capacity(&self) -> Vec<String> {
        lines(&self.root.join("capacity.log"))
    }

    fn ssh(&self) -> Vec<String> {
        lines(&self.root.join("ssh.log"))
    }

    fn checks(&self) -> Vec<String> {
        lines(&self.root.join("check.log"))
    }

    fn landed(&self) -> String {
        git(&self.origin, &["rev-parse", "main"])
    }
}

fn text(output: &Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

const RECORDING_CHECK: &str = "echo \"$PWD $SAFE_PUSH_LAND_HEAD $(git rev-parse HEAD) $(test -d target && echo warm || echo cold)\" >>\"$CHECK_LOG\"; mkdir -p target; touch target/built stray; test \"$(cat verdict)\" = ok";

#[test]
fn a_held_land_check_runs_on_a_host_with_room_from_a_bundle_and_leaves_only_the_shared_target() {
    let landing = Landing::new(RECORDING_CHECK, false);
    landing.gaming(true);
    landing.room("gex45", 40);
    let head = landing.lane_commit("ok");

    let output = landing.land();

    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("offloading to gex45"), "{}", text(&output));
    assert_eq!(landing.landed(), head);
    assert_eq!(landing.capacity(), ["offloaded"], "the local host admits only the light part while it is gaming-held");
    let slot = landing.root.join("remotes/gex45/.cache/land-offload/code-proj-main");
    assert_eq!(landing.checks(), [format!("{} {head} {head} cold", slot.display())]);
    assert!(slot.join("target/built").exists(), "the shared target directory persists");
    assert!(!slot.join("stray").exists(), "a file the check left is cleaned up");
    assert!(!slot.parent().unwrap().join("code-proj-main.bundle").exists(), "the bundle is removed");
    assert_eq!(git(&landing.remote, &["for-each-ref", "refs/land-offload"]), "", "the shipped ref is removed");
    assert_eq!(git(&landing.repo, &["for-each-ref", "refs/land-offload"]), "", "the local ref is removed");

    let second = landing.lane_commit("ok");
    let output = landing.land();
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(landing.landed(), second);
    assert!(landing.checks()[1].ends_with("warm"), "the second offloaded check reuses the first one's target: {:?}", landing.checks());
}

#[test]
fn the_remote_checks_exit_status_is_the_verdict() {
    let landing = Landing::new("exit 23", false);
    landing.gaming(true);
    landing.room("gex45", 40);
    let before = landing.landed();
    landing.lane_commit("ok");

    let output = landing.land();

    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains("exited 23"), "{}", text(&output));
    assert!(text(&output).contains("offloading to gex45"), "{}", text(&output));
    assert_eq!(landing.capacity(), ["offloaded"]);
    assert_eq!(landing.ssh().last().map(String::as_str), Some("gex45"));
    assert_eq!(landing.landed(), before);
}

#[test]
fn a_host_that_is_not_gaming_runs_its_own_check_without_asking_any_host() {
    let landing = Landing::new(RECORDING_CHECK, false);
    landing.gaming(false);
    landing.room("gex45", 40);
    let head = landing.lane_commit("ok");

    let output = landing.land();

    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(landing.capacity(), ["held"]);
    assert_eq!(landing.ssh(), Vec::<String>::new());
    assert!(landing.checks()[0].starts_with(&landing.repo.display().to_string()), "{:?}", landing.checks());
    assert_eq!(landing.landed(), head);
}

#[test]
fn no_host_with_cpu_memory_and_twenty_gib_of_disk_keeps_the_check_here() {
    let landing = Landing::new(RECORDING_CHECK, false);
    landing.gaming(true);
    landing.room("gex45", 17);
    let head = landing.lane_commit("ok");

    let output = landing.land();

    assert!(output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("gex45: 17.0 GiB disk free, needs 20"), "{}", text(&output));
    assert!(text(&output).contains("nexus: unreachable"), "{}", text(&output));
    assert_eq!(landing.capacity(), ["held"]);
    assert!(landing.checks()[0].starts_with(&landing.repo.display().to_string()), "{:?}", landing.checks());
    assert_eq!(landing.landed(), head);
}

#[test]
fn a_check_marked_local_only_is_never_offloaded() {
    let landing = Landing::new(RECORDING_CHECK, true);
    landing.gaming(true);
    landing.room("gex45", 40);
    let head = landing.lane_commit("ok");

    let output = landing.land();

    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(landing.capacity(), ["held"]);
    assert_eq!(landing.ssh(), Vec::<String>::new(), "no host is even asked");
    assert!(landing.checks()[0].starts_with(&landing.repo.display().to_string()), "{:?}", landing.checks());
    assert_eq!(landing.landed(), head);
}

#[test]
fn a_host_that_goes_dark_after_the_room_check_refuses_the_landing_instead_of_running_it_here() {
    let landing = Landing::new(RECORDING_CHECK, false);
    landing.gaming(true);
    landing.room("gex45", 40);
    fs::remove_dir_all(landing.root.join("remotes/gex45")).unwrap();
    let before = landing.landed();
    landing.lane_commit("ok");

    let output = landing.land();

    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
    assert!(text(&output).contains("NOT pushed"), "{}", text(&output));
    assert_eq!(landing.capacity(), ["offloaded"]);
    assert_eq!(landing.checks(), Vec::<String>::new(), "the check never ran here");
    assert_eq!(landing.landed(), before);
}
