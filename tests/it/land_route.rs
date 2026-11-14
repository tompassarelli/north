use north_core::foundation::shim;
use north_landing::land::queue::{self, Ticket};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NORTH: &str = "#!/bin/sh\ncase \"$1 $2\" in \"after-landing \"*|\"land train\") exit 2 ;; esac\nexec \"$REAL_NORTH\" \"$@\"\n";

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
        shim::write_executable(&bin.join("betterleaks"), "#!/bin/sh\nexit 0\n");
        shim::write_executable(&bin.join("north"), NORTH);
        crate::safe_push::link(&bin);
        fixture
    }

    fn top(&self) -> &Path {
        self.root.path()
    }

    fn repo(&self) -> PathBuf {
        self.top().join("repo")
    }

    fn fleet(&self, text: &str) {
        let snapshot = self.top().join("home/code/south/main/agents");
        fs::create_dir_all(&snapshot).unwrap();
        fs::write(snapshot.join("orchestration.toml"), text).unwrap();
    }

    fn write_config(&self, name: &str, landing: &str) {
        let text = format!("[init]\n\tdefaultBranch = main\n[user]\n\tname = safe-push-test\n\temail = safe-push-test@example.invalid\n[safe-push]\n\tallowEmail = safe-push-test@example.invalid\n{landing}");
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
        let output = Command::new(self.top().join("bin/safe-push"))
            .args(arguments)
            .current_dir(self.repo())
            .env("PATH", path)
            .env("REAL_NORTH", crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .env("HOME", self.top().join("home"))
            .env("AGENTS_ORCHESTRATION", self.top().join("orchestration.toml"))
            .env("XDG_STATE_HOME", self.top().join("state"))
            .env("NORTH_TEST_ROOT", self.top())
            .env("GIT_CONFIG_GLOBAL", self.top().join(config))
            .env_remove("XDG_CONFIG_HOME")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("SAFE_PUSH_DENYLIST")
            .env_remove("SAFE_PUSH_SOUTH")
            .env("THREADS_DB", self.top().join("threads.db"))
            .env("SAFE_PUSH_LAND_OFFLOAD", "0")
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

    fn graph_split(&self) -> Run {
        let probe = self.top().join("probe");
        if !probe.exists() {
            fs::create_dir_all(&probe).unwrap();
            self.git_at(&probe, &["init", "-q", "-b", "main"]);
            fs::write(probe.join("Cargo.toml"), "[package]\nname = \"probe\"\n").unwrap();
            self.git_at(&probe, &["add", "Cargo.toml"]);
            self.git_at(&probe, &["commit", "-qm", "probe"]);
            self.git_at(&probe, &["remote", "add", "origin", "https://example.invalid/owner/probe.git"]);
            fs::write(self.top().join("task.txt"), "split the probe\n").unwrap();
            fs::create_dir_all(self.top().join("planner")).unwrap();
            shim::write_executable(&self.top().join("planner/claude"), "#!/bin/sh\nexit 1\n");
        }
        let output = Command::new(crate::support::bin_exe!("CARGO_BIN_EXE_north"))
            .args(["graph", "split", "--base", "main", "--task"])
            .arg(self.top().join("task.txt"))
            .arg("--record")
            .arg(self.top().join("split.json"))
            .current_dir(&probe)
            .env("PATH", format!("{}:{}", self.top().join("planner").display(), std::env::var("PATH").unwrap_or_default()))
            .env("AGENTS_CLAUDE_BIN", self.top().join("planner/claude"))
            .env("HOME", self.top().join("home"))
            .env("AGENTS_ORCHESTRATION", self.top().join("orchestration.toml"))
            .env("XDG_STATE_HOME", self.top().join("state"))
            .env("GIT_CONFIG_GLOBAL", self.top().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
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
fn a_landing_the_lander_cannot_take_is_refused_naming_why_and_pushes_and_queues_nothing() {
    let fixture = Fixture::lander_queue();
    let host = local_host();
    let without_north = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).filter(|directory| !directory.join("north").exists()).collect::<Vec<_>>();
    let system = std::env::join_paths(without_north).unwrap().to_string_lossy().into_owned();
    let refused = |arguments: &[&str], expected: &str| {
        let main_before = fixture.main();
        let run = fixture.safe_push_on(arguments, "gitconfig", &system);
        assert_ne!(run.status, 0, "{}", run.output);
        assert!(run.output.contains(expected), "expected {expected:?}: {}", run.output);
        assert_eq!(fixture.main(), main_before, "a refused landing pushed main");
        assert!(fixture.tickets(&fixture.queue()).is_empty(), "{}", run.output);
    };
    refused(&["--to", "main", "--direct"], "--direct is refused");
    refused(&["--to", "main"], &format!("no lander serves this repository on {host}: north-land@repo.service is not declared"));
    fixture.settle("[landing]\n\tqueue = lander\n\tlander-host = another-host\n", "another lander host");
    fixture.work("work from an unnamed host");
    refused(&["--to", "main"], &format!("{host} is neither landing.lander-host nor in landing.remote-hosts nor an ssh table of the fleet orchestration.toml hosts for main, so the lander cannot take this lane; add {host} to landing.remote-hosts"));
    let snapshot = fixture.top().join("home/.local/state/north/shared-config");
    fs::create_dir_all(snapshot.join("current")).unwrap();
    fs::write(snapshot.join("stamp"), format!("{} 1000\n", "a".repeat(40))).unwrap();
    refused(&["--to", "main"], &format!("{host} is neither landing.lander-host nor in landing.remote-hosts for main, and the fleet orchestration.toml hosts could not be read here: shared configuration is stale: last fetch of south origin/main (aaaaaaaaaa) was "));
    refused(&["--to", "main"], "past the 60 min bound, though the fleet tick (north fleet tick) refreshes it every 5 min");
    fs::remove_dir_all(&snapshot).unwrap();
    fixture.settle(&format!("[landing]\n\tqueue = lock\n\tlander-host = {host}\n"), "a lock queue");
    fixture.work("work under the lock queue");
    refused(&["--to", "main"], "landing.queue = lock in .safe-push is no longer served: the lander lands every change to main");
}

#[test]
fn a_lane_that_removes_a_retired_landing_queue_from_dot_safe_push_is_queued_for_the_lander() {
    let fixture = Fixture::new();
    let host = local_host();
    fixture.settle("[landing]\n\tqueue = lock\n\tlander-host = another-host\n", "a retired lock queue");
    fixture.fleet(&format!("[hosts.{host}]\nssh = \"tom@{host}\"\n"));
    fs::write(fixture.repo().join(".safe-push"), "[landing]\n\tlander-host = another-host\n").unwrap();
    fixture.git(&["commit", "-qam", "drop the retired queue"]);
    let run = fixture.safe_push_on(&["--to", "main"], "gitconfig", &std::env::var("PATH").unwrap_or_default());
    assert_eq!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("Done: queued (outbox)"), "{}", run.output);
}

#[test]
fn a_lane_on_a_fleet_ssh_host_the_repository_does_not_list_is_queued_for_the_lander() {
    let fixture = Fixture::new();
    let host = local_host();
    let trusted = format!("[hosts.{host}]\nssh = \"tom@{host}\"\n[placement]\npublic_repos = [\"probe\"]\n");
    fixture.settle("[landing]\n\tqueue = lander\n\tlander-host = another-host\n", "a lander elsewhere");
    fixture.work("work from a fleet host");
    let run = fixture.safe_push_on(&["--to", "main"], "gitconfig", &std::env::var("PATH").unwrap_or_default());
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains(&format!("{host} is neither landing.lander-host")), "{}", run.output);
    fs::write(fixture.top().join("orchestration.toml"), &trusted).unwrap();
    let run = fixture.safe_push_on(&["--to", "main"], "gitconfig", &std::env::var("PATH").unwrap_or_default());
    assert_ne!(run.status, 0, "AGENTS_ORCHESTRATION chose the fleet file the route trusts: {}", run.output);
    let split = fixture.graph_split();
    assert!(split.output.contains("probe is not one"), "AGENTS_ORCHESTRATION chose the fleet file the graph split's public-repository gate trusts: {}", split.output);
    fixture.fleet(&trusted);
    let run = fixture.safe_push_on(&["--to", "main"], "gitconfig", &std::env::var("PATH").unwrap_or_default());
    assert_eq!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("Done: queued (outbox)"), "{}", run.output);
    let split = fixture.graph_split();
    assert!(!split.output.contains("probe is not one"), "{}", split.output);
}
