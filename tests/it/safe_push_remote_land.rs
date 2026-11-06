use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const LANDED: &str = "0123456789abcdef0123456789abcdef01234567";

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").current_dir(directory).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn real_git() -> String {
    let output = Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

struct Gex45 {
    _fixture: tempfile::TempDir,
    root: PathBuf,
}

impl Gex45 {
    fn new() -> Self {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().to_owned();
        let tools = root.join("tools");
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(root.join("home/.config/threads")).unwrap();
        fs::create_dir_all(root.join("home/.local/state/north/current")).unwrap();
        fs::write(root.join("home/.config/threads/remote"), "nexus\n").unwrap();
        fs::write(root.join("orchestration.toml"), "[hosts.gex45]\nledger_client = true\n\n[hosts.nexus]\nsettles_fleet = true\n").unwrap();
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), tools.join("north")).unwrap();
        crate::safe_push::link(&tools);
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_north"), root.join("home/.local/state/north/current/north")).unwrap();
        let trace = root.join("trace");
        shim::write_script(&tools.join("ssh"), &format!("printf 'ssh %s\\n' \"$*\" >>'{}'\ncat >'{}'\necho {LANDED}\n", trace.display(), root.join("received.mbox").display()));
        shim::write_script(&tools.join("git"), &format!("for word in \"$@\"; do [ \"$word\" != push ] || {{ printf 'git %s\\n' \"$*\" >>'{}'; break; }}; done\nexec '{}' \"$@\"\n", trace.display(), real_git()));
        shim::write_script(&tools.join("betterleaks"), "exit 0\n");
        shim::write_script(&tools.join("gh"), "case \"$1\" in repo) echo PUBLIC;; run) echo '[]';; api) printf 'pending\\tAutoland workflow queued\\thttps://example.test/run\\n';; *) exit 1;; esac\n");
        Gex45 { _fixture: fixture, root }
    }

    fn checkout(&self, name: &str, autoland: bool) -> (PathBuf, PathBuf) {
        let (repo, origin) = (self.root.join(name), self.root.join(format!("{name}.git")));
        git(&self.root, &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "Remote land scenario"]);
        git(&repo, &["config", "user.email", "land@example.invalid"]);
        git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        if autoland {
            fs::create_dir_all(repo.join(".github/workflows")).unwrap();
            fs::write(repo.join(".github/workflows/autoland.yml"), "on:\n  push:\n    branches: [\"claude/**\"]\n").unwrap();
            fs::write(repo.join(".safe-push"), "[landing]\n\tqueue = autoland\n").unwrap();
        }
        fs::write(repo.join("base"), "base\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        git(&repo, &["push", "-q", "origin", "main"]);
        git(&repo, &["switch", "-qc", "lane"]);
        fs::write(repo.join("change"), "lane change\n").unwrap();
        git(&repo, &["add", "change"]);
        git(&repo, &["commit", "-qm", "lane change for the ledger host"]);
        (repo, origin)
    }

    fn safe_push(&self, repo: &Path) -> Output {
        let mut paths = vec![self.root.join("tools")];
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
        Command::new(self.root.join("tools/safe-push"))
            .current_dir(repo)
            .args(["--to", "main"])
            .env("PATH", std::env::join_paths(paths).unwrap())
            .env("HOME", self.root.join("home"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("RESIDENT_CAP_HOST", "gex45")
            .env("SAFE_PUSH_AUTOLAND_POLL_SECONDS", "1")
            .env("SAFE_PUSH_AUTOLAND_WAIT_SECONDS", "1")
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("NORTH_SSH_CONFIG")
            .env_remove("SAFE_PUSH_TEST_REAL_NORTH")
            .output()
            .unwrap()
    }

    fn trace(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("trace")).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

#[test]
fn safe_push_on_a_ledger_client_lands_a_south_lane_through_ssh_nexus_south_land_with_no_git_push() {
    let host = Gex45::new();
    let (south, origin) = host.checkout("south", false);
    let before = git(&origin, &["rev-parse", "main"]);
    let output = host.safe_push(&south);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{stdout}{}", String::from_utf8_lossy(&output.stderr));
    assert!(stdout.contains(&format!("safe-push: landed {LANDED}")), "{stdout}");
    let trace = host.trace();
    for line in &trace {
        println!("{line}");
    }
    assert!(trace.iter().any(|line| line.starts_with("ssh ") && line.ends_with(" nexus south land")), "{trace:?}");
    assert!(!trace.iter().any(|line| line.starts_with("git ")), "{trace:?}");
    assert_eq!(git(&origin, &["rev-parse", "main"]), before);
    assert!(fs::read_to_string(host.root.join("received.mbox")).unwrap().contains("Subject: [PATCH] lane change for the ledger host"));
}

