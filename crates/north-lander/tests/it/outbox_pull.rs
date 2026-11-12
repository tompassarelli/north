use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use north_base::foundation::shim::write_executable;
use north_lander::land::heartbeat::Heartbeat;
use north_lander::land::jobs::Chores;
use north_lander::land::lander::Lander;
use north_lander::land::queue::{self, Ticket};

use super::serve::{Repository, World, git};

const CHECKOUT: &str = "NORTH_LAND_IT_OUTBOX_CHECKOUT";
const SSH: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done\nshift\n[ \"$1\" = -- ] && shift\nexec sh -c \"$*\"\n";
const REMOTE_SSH: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done\nhost=$1\nshift\n[ \"$1\" = -- ] && shift\nprintf '%s\\n' \"$host\" >> \"${FAKE_REMOTE_SSH_LOG:-/dev/null}\"\nexport XDG_STATE_HOME=\"$FAKE_REMOTE_STATE\"\nexec sh -c \"$*\"\n";

#[test]
fn outbox_puller() {
    let Ok(checkout) = std::env::var(CHECKOUT) else { return };
    let checkout = PathBuf::from(checkout);
    let origin = git(&checkout, &["remote", "get-url", "--push", "origin"]);
    let queue = queue::directory(&PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()), &origin);
    let lander = Lander { repo: checkout.clone(), worktree: checkout, offload_owner: String::new(), queue, checker: Vec::new(), app_token: Vec::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default() };
    for waiting in lander.pull_remote() {
        println!("waiting: {waiting}");
    }
}

fn outboxed(outbox: &Path, micros: u128, root: &Path, branch: &str, head: &str) -> String {
    let name = queue::file_name(micros, branch);
    let ticket = Ticket { origin: "https://github.com/tompassarelli/north".into(), root: root.to_owned(), host: "gex45".into(), branch: branch.into(), head: head.into(), session: Some(format!("{branch}-session")), queued_at: "2026-11-07T03:07:09Z".into(), queued_micros: Some(micros), ..Ticket::default() };
    queue::write(&outbox.join(&name), &ticket).unwrap();
    name
}

#[test]
fn an_outbox_ticket_whose_worktree_is_gone_is_withdrawn_and_the_ticket_behind_it_is_pulled_the_same_round() {
    let world = World::new();
    let repository = world.repository("north");
    write_executable(&world.path().join("bin/ssh"), SSH);
    git(&repository.checkout, &["config", "landing.remote-hosts", "gex45"]);
    let behind = repository.checkout.parent().unwrap().join("worktrees/behind");
    git(&repository.checkout, &["worktree", "add", "-q", "-b", "behind", behind.to_str().unwrap()]);
    git(&behind, &["commit", "-q", "--allow-empty", "-m", "behind"]);
    let outbox = repository.queue.join(queue::OUTBOX);
    let gone = outboxed(&outbox, 1_794_020_829_623_648, &repository.checkout.parent().unwrap().join("worktrees/n706-claim-filed"), "n706-claim-filed", "ef0ef324027acc6752dd7476b7b3dd49c13e768f");
    let pulled = outboxed(&outbox, 1_794_020_900_000_000, &behind, "behind", &git(&behind, &["rev-parse", "HEAD"]));

    let path = format!("{}:{}", world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "outbox_pull::outbox_puller", "--nocapture", "--test-threads", "1"])
        .env(CHECKOUT, &repository.checkout)
        .env("PATH", path)
        .env("HOME", world.path())
        .env("XDG_STATE_HOME", world.path().join("state"))
        .env("NORTH_TEST_ROOT", world.path())
        .env("NORTH_LAND_NORTH", world.path().join("bin/north"))
        .env("GIT_CONFIG_GLOBAL", world.path().join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let transcript = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    assert!(output.status.success() && !transcript.contains("waiting: "), "the outbox was skipped:\n{transcript}");
    let withdrawn = queue::read(&repository.queue.join(queue::WITHDRAWN).join(&gone)).unwrap_or_else(|error| panic!("{error}\n{transcript}"));
    assert!(withdrawn.verdict.unwrap_or_default().contains("is gone from gex45"), "{transcript}");
    assert!(outbox.join(queue::WITHDRAWN).join(&gone).is_file() && !outbox.join(&gone).exists(), "{transcript}");
    assert!(repository.queue.join(&pulled).is_file(), "the ticket behind it waited:\n{transcript}");
    let told = std::fs::read_to_string(world.path().join("bin/north.calls")).unwrap_or_default();
    assert!(told.lines().any(|call| call.starts_with("fleet send --from service:north-land n706-claim-filed-session ") && call.contains("was withdrawn from gex45's outbox")), "{told}");
}

struct Pull {
    world: World,
    repository: Repository,
    remote: PathBuf,
    calls: PathBuf,
    key: String,
    fleet: tempfile::TempDir,
}

impl Pull {
    fn new() -> Self {
        let world = World::new();
        let repository = world.repository("north");
        write_executable(&world.path().join("bin/ssh"), REMOTE_SSH);
        let fleet = tempfile::tempdir().unwrap();
        std::fs::write(fleet.path().join("orchestration.toml"), "[hosts.gex45]\nssh = \"gex45\"\n").unwrap();
        Self { fleet, remote: world.path().join("remote-state"), calls: world.path().join("ssh.calls"), key: repository.queue.file_name().unwrap().to_string_lossy().into_owned(), repository, world }
    }

    fn process(&self, name: &str) -> Command {
        let path = format!("{}:{}", self.world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", name, "--nocapture", "--test-threads", "1"])
            .env(CHECKOUT, &self.repository.checkout)
            .env("PATH", path)
            .env("HOME", self.world.path())
            .env("XDG_STATE_HOME", self.world.path().join("state"))
            .env("FAKE_REMOTE_STATE", &self.remote)
            .env("FAKE_REMOTE_SSH_LOG", &self.calls)
            .env("AGENTS_ORCHESTRATION", self.fleet.path().join("orchestration.toml"))
            .env("NORTH_LAND_NORTH", self.world.path().join("bin/north"))
            .env("GIT_CONFIG_GLOBAL", self.world.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("NORTH_TEST_ROOT")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn pull(&self) -> String {
        let output = self.process("outbox_pull::outbox_puller").output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    }
}

#[test]
fn a_pull_from_a_host_with_no_outbox_leaves_no_queue_in_that_hosts_state() {
    let pull = Pull::new();
    let transcript = pull.pull();

    assert!(std::fs::read_to_string(&pull.calls).is_ok_and(|calls| calls.contains("gex45")), "the pull never asked gex45:\n{transcript}");
    assert!(!pull.remote.join("north/land-queue").join(&pull.key).exists(), "a host with no outbox for {} gained its queue:\n{transcript}", pull.key);
}

const READY: &str = "NORTH_LAND_IT_VERDICTS_READY";
const WAITED: &str = "NORTH_LAND_IT_WAITED_TICKET";

#[test]
fn verdict_sender() {
    let (Ok(checkout), Ok(ready)) = (std::env::var(CHECKOUT), std::env::var(READY)) else { return };
    let checkout = PathBuf::from(checkout);
    let origin = git(&checkout, &["remote", "get-url", "--push", "origin"]);
    let queue = queue::directory(&PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()), &origin);
    let lander = Lander { repo: checkout.clone(), worktree: checkout, offload_owner: String::new(), queue, checker: Vec::new(), app_token: Vec::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default() };
    let _sending = north_lander::land::serve::verdicts::watch(&lander);
    std::fs::write(&ready, "").unwrap();
    let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
}

#[test]
fn hand_in_waiter() {
    let Ok(path) = std::env::var(WAITED) else { return };
    let path = PathBuf::from(path);
    let ticket = queue::read(&path).unwrap();
    let queued = north_lander::land::enqueue::Queued { path, outbox: true, ticket: ticket.clone(), squash: north_lander::landing::lane_squash::Squash::Unchanged };
    std::process::exit(i32::from(north_lander::land::enqueue::hand_in(&ticket.root, &queued, &PathBuf::from(std::env::var("XDG_STATE_HOME").unwrap()))));
}

#[test]
fn a_refusal_reaches_the_hand_in_on_the_lanes_host_within_two_seconds_with_no_records_mirror() {
    let pull = Pull::new();
    pull.repository.lane("conflicted");
    let (lander_copy, ticket) = queue::pending(&pull.repository.queue).unwrap().pop().unwrap();
    let ticket = Ticket { host: "gex45".into(), ..ticket };
    queue::write(&lander_copy, &ticket).unwrap();
    north_base::land_ticket::queued(&lander_copy, "taken from gex45's outbox").unwrap();
    let outboxed = queue::directory(&pull.remote, &ticket.origin).join(queue::OUTBOX).join(lander_copy.file_name().unwrap());
    queue::write(&outboxed, &ticket).unwrap();

    let ready = pull.world.path().join("verdicts.ready");
    let mut sender = pull.process("outbox_pull::verdict_sender");
    sender.env(READY, &ready).stdin(Stdio::piped());
    let mut sender = sender.spawn().unwrap();
    let mut waiter = pull.process("outbox_pull::hand_in_waiter");
    waiter.env(WAITED, &outboxed).env("XDG_STATE_HOME", &pull.remote);
    let mut waiter = waiter.spawn().unwrap();
    assert!(north_base::platform::wait::until(Duration::from_secs(20), || ready.exists()), "the lander never started sending verdicts");

    let verdict = "conflict with main at conflicted.txt; rebase onto origin/main and hand in again";
    queue::decide(&lander_copy, ticket, verdict.to_owned()).unwrap();
    let answered = north_base::platform::wait::until(Duration::from_secs(2), || waiter.try_wait().unwrap().is_some());
    if !answered {
        let _ = waiter.kill();
    }
    let _ = sender.kill();
    let _ = sender.wait();
    let output = waiter.wait_with_output().unwrap();
    let transcript = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(answered, "the hand-in on gex45 heard nothing within 2 s of the lander's refusal:\n{transcript}");
    assert!(output.status.code() == Some(1) && transcript.contains(verdict), "{transcript}");
}

#[test]
fn an_outbox_ticket_carrying_its_lanes_bundle_is_queued_without_fetching_the_lane_from_its_host() {
    let world = World::new();
    let repository = world.repository("north");
    write_executable(&world.path().join("bin/ssh"), SSH);
    git(&repository.checkout, &["config", "landing.remote-hosts", "gex45"]);
    let elsewhere = world.path().join("gex45/worktrees/bundled");
    git(world.path(), &["clone", "-q", repository.origin.to_str().unwrap(), elsewhere.to_str().unwrap()]);
    git(&elsewhere, &["switch", "-q", "-c", "bundled"]);
    git(&elsewhere, &["commit", "-q", "--allow-empty", "-m", "bundled"]);
    let head = git(&elsewhere, &["rev-parse", "HEAD"]);
    let outbox = repository.queue.join(queue::OUTBOX);
    let name = outboxed(&outbox, 1_794_441_825_980_748, &elsewhere, "bundled", &head);
    let bundle = queue::bundle(&outbox.join(&name));
    git(&elsewhere, &["bundle", "create", "-q", bundle.to_str().unwrap(), "refs/heads/bundled", "^origin/main"]);
    std::fs::remove_dir_all(&elsewhere).unwrap();

    let path = format!("{}:{}", world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "outbox_pull::outbox_puller", "--nocapture", "--test-threads", "1"])
        .env(CHECKOUT, &repository.checkout)
        .env("PATH", path)
        .env("HOME", world.path())
        .env("XDG_STATE_HOME", world.path().join("state"))
        .env("NORTH_TEST_ROOT", world.path())
        .env("NORTH_LAND_NORTH", world.path().join("bin/north"))
        .env("GIT_CONFIG_GLOBAL", world.path().join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    let transcript = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));

    assert!(repository.queue.join(&name).is_file(), "the bundled ticket was not queued:\n{transcript}");
    assert_eq!(git(&repository.checkout, &["rev-parse", "refs/land-remote/gex45/bundled"]), head, "{transcript}");
    assert!(!outbox.join(&name).exists() && !bundle.exists(), "the taken ticket or its bundle stayed in the outbox:\n{transcript}");
}
