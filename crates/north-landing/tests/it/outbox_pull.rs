use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;

use north_base::foundation::shim::write_executable;
use north_landing::land::heartbeat::Heartbeat;
use north_landing::land::jobs::Chores;
use north_landing::land::lander::Lander;
use north_landing::land::queue::{self, Ticket};

use super::serve::{World, git};

const CHECKOUT: &str = "NORTH_LAND_IT_OUTBOX_CHECKOUT";
const SSH: &str = "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o|-p) shift 2 ;; -*) shift ;; *) break ;; esac; done\nshift\n[ \"$1\" = -- ] && shift\nexec sh -c \"$*\"\n";

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
