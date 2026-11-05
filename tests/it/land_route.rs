use north_core::foundation::shim;
use north_landing::land::queue::{self, Ticket};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NORTH: &str = "#!/bin/sh\ncase \"$1\" in after-landing|land-train) exit 2 ;; esac\nexec \"$REAL_NORTH\" \"$@\"\n";

struct Fixture {
    root: tempfile::TempDir,
}

struct Run {
    status: i32,
    output: String,
}

fn local_host() -> String {
    let host = fs::read_to_string("/proc/sys/kernel/hostname").unwrap();
    host.trim().split('.').next().unwrap().to_owned()
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self { root: tempfile::tempdir().unwrap() };
        let top = fixture.top().to_path_buf();
        fixture.write_config("gitconfig", "");
        fixture.git_at(&top, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        fixture.git_at(&top, &["init", "-q", "-b", "main", "repo"]);
        fs::write(fixture.repo().join("content.txt"), "initial\n").unwrap();
        fixture.git(&["add", "content.txt"]);
        fixture.git(&["commit", "-qm", "initial"]);
        fixture.git(&["remote", "add", "origin", &top.join("remote.git").to_string_lossy()]);
        fixture.git(&["push", "-q", "-u", "origin", "main"]);
        fixture.git(&["commit", "--allow-empty", "-qm", "outgoing"]);
        let bin = top.join("bin");
        fs::create_dir_all(&bin).unwrap();
        shim::write_executable(&bin.join("gitleaks"), "#!/bin/sh\nexit 0\n");
        shim::write_executable(&bin.join("north"), NORTH);
        fixture
    }

    fn top(&self) -> &Path {
        self.root.path()
    }

    fn repo(&self) -> PathBuf {
        self.top().join("repo")
    }

    fn write_config(&self, name: &str, landing: &str) {
        let text = format!("[init]\n\tdefaultBranch = main\n[user]\n\tname = safe-push-test\n\temail = safe-push-test@example.invalid\n{landing}");
        fs::write(self.top().join(name), text).unwrap();
    }

    fn git_at(&self, directory: &Path, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(directory)
            .env("GIT_CONFIG_GLOBAL", self.top().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn git(&self, arguments: &[&str]) -> String {
        self.git_at(&self.repo(), arguments)
    }

    fn main(&self) -> String {
        self.git_at(&self.top().join("remote.git"), &["rev-parse", "main"])
    }

    fn settle(&self, settings: &str, message: &str) {
        fs::write(self.repo().join(".safe-push"), settings).unwrap();
        self.git(&["add", ".safe-push"]);
        self.git(&["commit", "-qm", message]);
        self.git(&["push", "-q", "origin", "HEAD:main"]);
        self.git_at(&self.top().join("remote.git"), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    }

    fn work(&self, line: &str) -> String {
        fs::write(self.repo().join("content.txt"), format!("{line}\n")).unwrap();
        self.git(&["commit", "-qam", line]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn queue(&self) -> PathBuf {
        queue::directory(&self.top().join("state"), &self.git(&["remote", "get-url", "--push", "origin"]))
    }

    fn tickets(&self, directory: &Path) -> Vec<Ticket> {
        queue::pending(directory).unwrap_or_default().into_iter().map(|(_, ticket)| ticket).collect()
    }

    fn safe_push_on(&self, arguments: &[&str], config: &str, system_path: &str) -> Run {
        let path = format!("{}:{system_path}", self.top().join("bin").display());
        let output = Command::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/safe-push"))
            .args(arguments)
            .current_dir(self.repo())
            .env("PATH", path)
            .env("REAL_NORTH", env!("CARGO_BIN_EXE_north"))
            .env("HOME", self.top().join("home"))
            .env("XDG_STATE_HOME", self.top().join("state"))
            .env("GIT_CONFIG_GLOBAL", self.top().join(config))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("SAFE_PUSH_DENYLIST", self.top().join("absent-denylist.toml"))
            .env("SAFE_PUSH_SOUTH", self.top().join("absent-south"))
            .env("THREADS_DB", self.top().join("threads.db"))
            .env("SAFE_PUSH_LAND_OFFLOAD", "0")
            .env_remove("SAFE_PUSH_TEST_REAL_NORTH")
            .env_remove("SAFE_PUSH_OUTPUT_CLAIMED")
            .env_remove("SAFE_PUSH_LOCK_HELD")
            .env_remove("NORTH_NO_AUTOREAP")
            .env_remove("NORTH_FLEET_SESSION")
            .env_remove("NORTH_LAND_SERVING")
            .env_remove("SAFE_PUSH_LAND_CAPACITY")
            .env_remove("SAFE_PUSH_LAND_OFFLOAD_HOST")
            .env_remove("SAFE_PUSH_LAND_RUNNER")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        Run { status: output.status.code().unwrap_or(-1), output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)) }
    }

    fn lander_queue() -> Self {
        let fixture = Self::new();
        fixture.settle(&format!("[landing]\n\tqueue = lander\n\tlander-host = {}\n\tremote-hosts = remote-host\n", local_host()), "lander queue");
        fixture.work("lander work");
        fixture
    }
}

#[test]
fn lander_queue_without_north_refuses_instead_of_pushing() {
    let fixture = Fixture::lander_queue();
    fs::remove_file(fixture.top().join("bin/north")).unwrap();
    let without_north = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|directory| !directory.join("north").exists()).collect::<Vec<_>>();
    let main_before = fixture.main();
    let run = fixture.safe_push_on(&["--to", "main"], "gitconfig", &std::env::join_paths(without_north).unwrap().to_string_lossy());
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("lacks north land route"), "{}", run.output);
    assert_eq!(fixture.main(), main_before, "a lander repository pushed main without north land route");
    assert!(fixture.tickets(&fixture.queue()).is_empty(), "{}", run.output);
}

