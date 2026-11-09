use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;

use north_base::foundation::shim::write_executable;
use north_lander::land::heartbeat::Heartbeat;
use north_lander::land::jobs::Chores;
use north_lander::land::lander::Lander;
use north_lander::land::queue::{self, Ticket};

use super::serve::{World, git};

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
    checkout: PathBuf,
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
        Self { fleet, remote: world.path().join("remote-state"), calls: world.path().join("ssh.calls"), key: repository.queue.file_name().unwrap().to_string_lossy().into_owned(), checkout: repository.checkout, world }
    }

    fn pull(&self, test_root: Option<&Path>) -> String {
        let path = format!("{}:{}", self.world.path().join("bin").display(), std::env::var("PATH").unwrap_or_default());
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "outbox_pull::outbox_puller", "--nocapture", "--test-threads", "1"])
            .env(CHECKOUT, &self.checkout)
            .env("PATH", path)
            .env("HOME", self.world.path())
            .env("XDG_STATE_HOME", self.world.path().join("state"))
            .env("FAKE_REMOTE_STATE", &self.remote)
            .env("FAKE_REMOTE_SSH_LOG", &self.calls)
            .env("AGENTS_ORCHESTRATION", self.fleet.path().join("orchestration.toml"))
            .env("NORTH_LAND_NORTH", self.world.path().join("bin/north"))
            .env("GIT_CONFIG_GLOBAL", self.world.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        match test_root {
            Some(root) => command.env("NORTH_TEST_ROOT", root),
            None => command.env_remove("NORTH_TEST_ROOT"),
        };
        let output = command.output().unwrap();
        format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
    }
}

#[test]
fn a_pull_from_a_host_with_no_outbox_leaves_no_queue_in_that_hosts_state() {
    let pull = Pull::new();
    let transcript = pull.pull(None);

    assert!(std::fs::read_to_string(&pull.calls).is_ok_and(|calls| calls.contains("gex45")), "the pull never asked gex45:\n{transcript}");
    assert!(!pull.remote.join("north/land-queue").join(&pull.key).exists(), "a host with no outbox for {} gained its queue:\n{transcript}", pull.key);
}

#[test]
fn a_test_fixture_pulls_no_outbox_from_a_real_fleet_host() {
    let pull = Pull::new();
    let transcript = pull.pull(Some(pull.world.path()));

    assert!(std::fs::read_to_string(&pull.calls).is_err(), "a test root still reached a real fleet host:\n{transcript}");
    assert!(!pull.remote.join("north/land-queue").join(&pull.key).exists(), "a host with no outbox for {} gained its queue:\n{transcript}", pull.key);
}
