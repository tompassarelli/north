use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOM: &str = "20\n1.50 1.00 1.00 2/900 1\nMemTotal:       65536000 kB\nMemAvailable:   40000000 kB\nFilesystem 1024-blocks Used Available Capacity Mounted on\n/dev/md1 490000000 400000000 %KIB% 90% /\n@ADMISSION@\n{\"profile\":\"unattended\",\"aggregateCpuLimit\":20,\"committedBatchCpus\":2,\"cpuSomeAvg10\":5}\n";
const GIB: u64 = 1024 * 1024;

const FAKE_SSH: &str = r#"while [ $# -gt 0 ]; do
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

const CAPACITY_STUB: &str = "printf '%s\\n' \"$1\" >>\"$CAPACITY_LOG\"\nshift 2\nexec \"$@\"\n";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn executable(path: &Path, body: &str) {
    shim::write_script(path, body);
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect()
}

struct Landing {
    _fixture: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    origin: PathBuf,
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
            config("land.local-only", &format!("needs this host :: {check}"));
        }
        fs::write(repo.join("verdict"), "ok\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "Land checks"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), remote.to_str().unwrap()]);
        git(&repo, &["switch", "-qc", "lane"]);
        executable(&tools.join("ssh"), FAKE_SSH);
        executable(&tools.join("capacity-stub"), CAPACITY_STUB);
        executable(&tools.join("betterleaks"), "exit 0\n");
        executable(&tools.join("gh"), "case \"$1\" in repo) echo PUBLIC;; run) echo '[]';; *) exit 1;; esac\n");
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), tools.join("north")).unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("home/.local/state/north/current/north")).unwrap();
        fs::write(
            root.join("orchestration.toml"),
            "[placement]\norder = [\"whiterabbit\", \"gex45\", \"nexus\"]\n[hosts.whiterabbit]\nssh = \"tom@whiterabbit\"\n[hosts.gex45]\nssh = \"tom@gex45\"\n[hosts.nexus]\nssh = \"tom@nexus\"\n",
        )
        .unwrap();
        Landing { _fixture: fixture, root, repo, origin }
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
        self.land_with_reserve(&self.root.join("no-game-reserve.json"))
    }

    fn land_with_reserve(&self, reserve: &Path) -> Output {
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
            .env("GAME_RESERVE_STATE", reserve)
            .env("FAKE_SSH_LOG", self.root.join("ssh.log"))
            .env("NORTH_OFFLOAD_TARGET_FLOOR_MIB", "0")
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

    fn landed(&self) -> String {
        git(&self.origin, &["rev-parse", "main"])
    }
}

fn text(output: &Output) -> String {
    format!("{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
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

