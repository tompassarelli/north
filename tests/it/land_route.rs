use north_core::foundation::shim;
use north_landing::land::hold;
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

    fn safe_push(&self, arguments: &[&str], config: &str) -> Run {
        self.safe_push_on(arguments, config, &std::env::var("PATH").unwrap_or_default())
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

    fn remote_host(&self) {
        self.settle(&format!("[landing]\n\tqueue = lander\n\tlander-host = fixture-host\n\tremote-hosts = remote-host {}\n", local_host()), "this host is a remote host");
    }

    fn neither_host(&self) {
        self.settle("[landing]\n\tqueue = lander\n\tlander-host = fixture-host\n\tremote-hosts = remote-host\n", "this host is neither lander host");
    }
}

fn queued(run: &Run) {
    assert_eq!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("Done: queued"), "{}", run.output);
    assert!(!run.output.contains("for secrets ("), "safe-push ran its own checks before queueing: {}", run.output);
}

#[test]
fn lander_queue() {
    let fixture = Fixture::lander_queue();
    let main_before = fixture.main();
    let run = fixture.safe_push(&["--to", "main"], "gitconfig");
    queued(&run);
    assert_eq!(fixture.tickets(&fixture.queue()).len(), 1, "the lander host did not queue exactly once: {}", run.output);
    assert_eq!(fixture.main(), main_before, "the lander host pushed main instead of queueing");

    fixture.remote_host();
    fixture.work("remote work");
    let main_before = fixture.main();
    let run = fixture.safe_push(&["--to", "main"], "gitconfig");
    queued(&run);
    assert!(run.output.contains("Done: queued (outbox)"), "a remote host did not queue in its outbox: {}", run.output);
    assert_eq!(fixture.tickets(&fixture.queue().join(queue::OUTBOX)).len(), 1, "a remote host did not queue: {}", run.output);
    assert_eq!(fixture.main(), main_before, "a remote host pushed main instead of queueing");

    fixture.neither_host();
    fixture.work("other work");
    let main_before = fixture.main();
    let host = local_host();
    for arguments in [&["--to", "main"][..], &["--to", "main", "--several-commits", "each lander commit is a bisect point"]] {
        let run = fixture.safe_push(arguments, "gitconfig");
        assert_ne!(run.status, 0, "{}", run.output);
        assert!(run.output.contains(&format!("{host} is neither landing.lander-host nor in landing.remote-hosts")), "{}", run.output);
        assert!(run.output.contains(&format!("add {host} to landing.remote-hosts")), "{}", run.output);
    }
    assert_eq!(fixture.tickets(&fixture.queue()).len(), 1, "a host outside the lander settings queued a ticket");
    assert_eq!(fixture.tickets(&fixture.queue().join(queue::OUTBOX)).len(), 1, "a host outside the lander settings queued a ticket");
    assert_eq!(fixture.main(), main_before, "a refused host pushed main");
}

#[test]
fn lander_queue_direct() {
    let fixture = Fixture::lander_queue();
    let main_before = fixture.main();
    let run = fixture.safe_push(&["--to", "main", "--direct"], "gitconfig");
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("--direct is refused here: landing.queue is lander"), "{}", run.output);
    assert_eq!(fixture.main(), main_before, "--direct pushed main from the lander host");
    fixture.neither_host();
    fixture.work("other work");
    let main_before = fixture.main();
    let run = fixture.safe_push(&["--to", "main", "--direct"], "gitconfig");
    assert_ne!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("--direct is refused here: landing.queue is lander"), "{}", run.output);
    assert!(fixture.tickets(&fixture.queue()).is_empty(), "--direct queued a ticket");
    assert!(fixture.tickets(&fixture.queue().join(queue::OUTBOX)).is_empty(), "--direct queued a ticket");
    let direct_log = fixture.top().join("state/safe-push/direct-landings.tsv");
    assert!(!fs::read_to_string(direct_log).unwrap_or_default().contains('\t'), "--direct logged a landing in a lander repository");
    assert_eq!(fixture.main(), main_before, "--direct pushed main");
}

#[test]
fn lander_queue_several_commits() {
    let fixture = Fixture::lander_queue();
    fixture.remote_host();
    fixture.work("second lander work");
    let lane = fixture.git(&["rev-parse", "HEAD"]);
    let main_before = fixture.main();
    let reason = "each lander commit is a bisect point";
    let run = fixture.safe_push(&["--to", "main", "--several-commits", reason], "gitconfig");
    queued(&run);
    let tickets = fixture.tickets(&fixture.queue().join(queue::OUTBOX));
    assert_eq!(tickets.len(), 1, "{}", run.output);
    assert_eq!(tickets[0].several_commits.as_deref(), Some(reason), "--several-commits did not queue a ticket carrying its reason");
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), lane, "--several-commits squashed the lane before queueing");
    let several_log = fixture.top().join("state/safe-push/several-commit-landings.tsv");
    assert!(!fs::read_to_string(several_log).unwrap_or_default().contains(reason), "--several-commits landed through the bash path");
    assert_eq!(fixture.main(), main_before, "a lander repository case pushed main");
}

#[test]
fn lander_queue_keep_lane() {
    let fixture = Fixture::lander_queue();
    let main_before = fixture.main();
    let run = fixture.safe_push(&["--to", "main", "--keep-lane"], "gitconfig");
    queued(&run);
    let tickets = fixture.tickets(&fixture.queue());
    assert_eq!(tickets.len(), 1, "{}", run.output);
    assert!(tickets[0].keep_lane, "--keep-lane did not reach the ticket");
    assert_eq!(fixture.main(), main_before, "--keep-lane pushed main");
}

#[test]
fn lander_default() {
    let fixture = Fixture::new();
    fixture.git_at(&fixture.top().join("remote.git"), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fixture.write_config("machine-gitconfig", &format!("[landing]\n\tlander-host = {}\n", local_host()));
    fixture.write_config("other-gitconfig", "[landing]\n\tlander-host = fixture-host\n");
    fixture.work("default work");
    let main_before = fixture.main();
    let lander = hold::take(&fixture.queue(), &fixture.repo()).unwrap();
    let run = fixture.safe_push(&["--to", "main"], "machine-gitconfig");
    queued(&run);
    assert_eq!(fixture.tickets(&fixture.queue()).len(), 1, "a repository with no landing.queue did not queue on the machine's lander host: {}", run.output);
    assert_eq!(fixture.main(), main_before, "a repository with no landing.queue pushed main instead of queueing");
    let run = fixture.safe_push(&["--to", "main"], "other-gitconfig");
    assert_eq!(run.status, 0, "{}", run.output);
    assert_eq!(fixture.main(), fixture.git(&["rev-parse", "HEAD"]), "a host that is not the lander host did not land per lane: {}", run.output);
    assert_eq!(fixture.tickets(&fixture.queue()).len(), 1, "a host that is not the lander host queued a ticket");
    drop(lander);
    fixture.settle("[landing]\n\tqueue = lock\n", "opt out of the lander");
    fixture.work("opted-out work");
    let opted_out_before = fixture.main();
    fs::remove_dir_all(fixture.queue()).unwrap();
    let run = fixture.safe_push(&["--to", "main"], "machine-gitconfig");
    assert_eq!(run.status, 0, "{}", run.output);
    assert!(fixture.tickets(&fixture.queue()).is_empty(), "landing.queue = lock queued a ticket: {}", run.output);
    assert_ne!(fixture.main(), opted_out_before, "landing.queue = lock did not land per lane: {}", run.output);
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

#[test]
fn lander_default_unserved() {
    let fixture = Fixture::new();
    fixture.git_at(&fixture.top().join("remote.git"), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fixture.write_config("machine-gitconfig", &format!("[landing]\n\tlander-host = {}\n", local_host()));
    fixture.work("unserved work");
    let run = fixture.safe_push(&["--to", "main"], "machine-gitconfig");
    assert_eq!(run.status, 0, "{}", run.output);
    assert!(run.output.contains("no lander serves this repository's queue"), "{}", run.output);
    assert_eq!(fixture.main(), fixture.git(&["rev-parse", "HEAD"]), "a repository no lander serves did not land per lane: {}", run.output);
    assert!(fixture.tickets(&fixture.queue()).is_empty(), "a repository no lander serves queued a ticket that would never land: {}", run.output);
}
