use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use north_base::platform::wait;
use north_lander::land::queue::{self, Ticket};

const ARGUMENTS: &str = "NORTH_LAND_IT_ARGUMENTS";
pub(super) const HOST: &str = "land-test-host";
pub(super) const HANG: Duration = Duration::from_secs(3600);
static TICKET_CLOCK: AtomicU64 = AtomicU64::new(1_790_000_000_000_000);
const TOKEN: &str = "#!/bin/sh\necho app-token\n";
const GREEN: &str = "#!/bin/sh\n[ -z \"$HOLD\" ] || { : > \"$HOLD.started\"; while [ ! -e \"$HOLD.release\" ]; do sleep 0.05; done; }\necho 'safe-push: verdict green'\n";

#[test]
fn lander_process() {
    let Ok(arguments) = std::env::var(ARGUMENTS) else { return };
    let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
    std::process::exit(i32::from(north_lander::land::run(&arguments)));
}

#[test]
fn enqueue_process() {
    let Ok(arguments) = std::env::var(ARGUMENTS) else { return };
    let arguments: Vec<String> = arguments.split('\n').map(str::to_owned).collect();
    std::process::exit(i32::from(north_lander::land::enqueue::run(&arguments)));
}

#[test]
fn a_busy_lander_moves_to_a_promoted_link_within_its_bound() {
    let world = World::new();
    let repository = world.repository("north");
    let old = runtime_commit(&repository, "old");
    let new = runtime_commit(&repository, "new");
    std::os::unix::fs::symlink(format!("runtimes/{old}"), world.path().join("lander")).unwrap();
    repository.lane("stale");
    let (path, mut ticket) = queue::pending(&repository.queue).unwrap().pop().unwrap();
    ticket.alone_green = None;
    ticket.alone_base = None;
    queue::write(&path, &ticket).unwrap();

    let (checker, token) = (world.path().join("bin/checker"), world.path().join("bin/token"));
    let hold = world.path().join("hold");
    let mut command = world.land(&["serve", "--repo", repository.checkout.to_str().unwrap(), "--checker", checker.to_str().unwrap(), "--app-token", token.to_str().unwrap()]);
    command.env("HOLD", &hold);
    let mut child = command.spawn().unwrap();
    let checking = || world.path().join("hold.started").exists();
    wait::until(HANG, || checking() || child.try_wait().unwrap().is_some());
    if !checking() {
        let _ = child.kill();
        let output = child.wait_with_output().unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        panic!("the lander never started checking the stale lane:\n{text}");
    }
    std::fs::remove_file(world.path().join("lander")).unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{new}"), world.path().join("lander")).unwrap();
    let exited = wait::until(HANG, || child.try_wait().unwrap().is_some());
    std::fs::write(world.path().join("hold.release"), "").unwrap();
    if !exited {
        let _ = child.kill();
    }
    let output = child.wait_with_output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(exited && output.status.success(), "the lander announced the runtime change and kept waiting for the check in flight instead of exiting:\n{text}");
    assert!(text.contains("north land: the live runtime changed"), "{text}");
}

pub(super) fn git(dir: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(arguments).env("GIT_CONFIG_GLOBAL", dir.ancestors().find(|path| path.join("gitconfig").is_file()).unwrap().join("gitconfig")).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {arguments:?} in {}: {}", dir.display(), String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(super) fn runtime_commit(repository: &Repository, text: &str) -> String {
    let path = repository.checkout.join("crates/north-ops/src/runtime-fixture.txt");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, text).unwrap();
    git(&repository.checkout, &["add", "-A"]);
    git(&repository.checkout, &["commit", "-qm", text]);
    git(&repository.checkout, &["rev-parse", "HEAD"])
}

pub(super) struct World {
    root: tempfile::TempDir,
}

pub(super) struct Repository {
    pub(super) origin: PathBuf,
    pub(super) checkout: PathBuf,
    pub(super) queue: PathBuf,
}

impl World {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("gitconfig"), "[user]\n\tname = t\n\temail = t@example.com\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n").unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("checker"), GREEN).unwrap();
        std::fs::write(bin.join("north"), "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$0.calls\"\n").unwrap();
        std::fs::write(bin.join("token"), TOKEN).unwrap();
        for program in ["checker", "north", "token"] {
            Command::new("chmod").arg("+x").arg(bin.join(program)).status().unwrap();
        }
        Self { root }
    }

    pub(super) fn path(&self) -> &Path {
        self.root.path()
    }

    pub(super) fn repository(&self, name: &str) -> Repository {
        let project = self.path().join(name);
        let origin = project.join("origin.git");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        let checkout = project.join("main");
        git(&project, &["clone", "-q", origin.to_str().unwrap(), "main"]);
        for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
            git(&checkout, &["config", key, value]);
        }
        std::fs::write(checkout.join("README"), "start\n").unwrap();
        std::fs::write(checkout.join(".safe-push"), "[landing]\n\tpush-app = lander\n").unwrap();
        git(&checkout, &["add", "-A"]);
        git(&checkout, &["commit", "-q", "-m", "start"]);
        git(&checkout, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        let queue = queue::directory(&self.path().join("state"), &git(&checkout, &["remote", "get-url", "--push", "origin"]));
        Repository { origin, checkout, queue }
    }

    pub(super) fn lander(&self, repository: &Repository, hold: Option<&Path>) -> Child {
        self.serve_command(repository, hold).spawn().unwrap()
    }

    pub(super) fn serve_command(&self, repository: &Repository, hold: Option<&Path>) -> Command {
        let mut command = self.land(&["serve", "--once", "--repo", repository.checkout.to_str().unwrap(), "--checker", self.path().join("bin/checker").to_str().unwrap(), "--app-token", self.path().join("bin/token").to_str().unwrap()]);
        if let Some(hold) = hold {
            command.env("HOLD", hold);
        }
        command
    }

    pub(super) fn land(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "serve::lander_process", "--nocapture", "--test-threads", "1"])
            .env(ARGUMENTS, arguments.join("\n"))
            .env("XDG_STATE_HOME", self.path().join("state"))
            .env("NORTH_TEST_ROOT", self.path())
            .env("NORTH_DECISION_LOG", self.path().join("state/north/decisions"))
            .env("HOME", self.path())
            .env("NORTH_LAND_NORTH", self.path().join("bin/north"))
            .env("NORTH_RUNTIME_LINK", self.path().join("lander"))
            .env("GIT_CONFIG_GLOBAL", self.path().join("gitconfig"))
            .env("RESIDENT_CAP_HOST", HOST)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("HOLD")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
}

impl Repository {
    pub(super) fn lane(&self, branch: &str) -> String {
        self.lane_with(branch, "origin/main", (&format!("{branch}.txt"), "lane\n"), (&format!("{branch} (north#360)"), None))
    }

    pub(super) fn lane_with(&self, branch: &str, from: &str, file: (&str, &str), commit: (&str, Option<&str>)) -> String {
        let (message, session) = commit;
        let lane = self.checkout.parent().unwrap().join("worktrees").join(branch);
        git(&self.checkout, &["worktree", "add", "-q", "-b", branch, lane.to_str().unwrap(), from]);
        std::fs::create_dir_all(lane.join(file.0).parent().unwrap()).unwrap();
        std::fs::write(lane.join(file.0), file.1).unwrap();
        git(&lane, &["add", "-A"]);
        git(&lane, &["commit", "-q", "-m", &format!("{message}\n\nIncident-test: none; a fixture lane the incident guard must not look up on GitHub")]);
        let micros = u128::from(TICKET_CLOCK.fetch_add(1, Ordering::Relaxed));
        let ticket = Ticket {
            origin: git(&lane, &["remote", "get-url", "--push", "origin"]),
            root: lane.clone(),
            host: HOST.to_owned(),
            branch: branch.to_owned(),
            head: git(&lane, &["rev-parse", "HEAD"]),
            session: session.map(str::to_owned),
            queued_at: "2026-11-05T00:00:00Z".to_owned(),
            first_commit_secs: None,
            several_commits: None,
            red_fix: None,
            urgent: None,
            urgent_at: None,
            lander_fix: None,
            decided_on: None,
            verdict: None,
            deferral: None,
            keep_lane: false,
            published: None,
            alone_green: Some(git(&lane, &["rev-parse", "HEAD"])),
            alone_base: Some(git(&lane, &["rev-parse", "origin/main"])),
            alone_unsettled: None,
            unjudged_on: None,
            gated_from: None,
            hand_in: None,
            queued_micros: Some(micros),
            tests_after_landing: false,
        };
        queue::write(&self.queue.join(queue::file_name(micros, branch)), &ticket).unwrap();
        ticket.head
    }

}

pub(super) fn report(output: &Output) -> String {
    format!("exit {:?}\nstdout:\n{}\nstderr:\n{}", output.status.code(), String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

