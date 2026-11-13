use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicU64;

use super::Serve;
use crate::land::candidate::{self, git};
use crate::land::heartbeat::Heartbeat;
use crate::land::jobs::Chores;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]).unwrap();
    git(root, &["commit", "-q", "-m", message]).unwrap();
    git(root, &["rev-parse", "HEAD"]).unwrap()
}

fn init(root: &Path, bare: bool) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-q", "-b", "main", if bare { "--bare" } else { "." }]).unwrap();
}

pub(in super::super) fn a_crate_split_sends_back_the_queued_lane_on_a_moved_file_in_the_same_round_naming_the_split() {
    let dir = tempfile::tempdir().unwrap();
    let (origin, repo) = (dir.path().join("origin.git"), dir.path().join("main"));
    init(&origin, true);
    init(&repo, false);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&repo, &["config", key, value]).unwrap();
    }
    git(&repo, &["remote", "add", "origin", &origin.display().to_string()]).unwrap();
    fs::create_dir_all(repo.join("crates/a/src")).unwrap();
    fs::write(repo.join("crates/a/src/x.rs"), "one\ntwo\nthree\n").unwrap();
    fs::write(repo.join("crates/a/src/y.rs"), "y\n").unwrap();
    commit(&repo, "base");
    git(&repo, &["push", "-q", "origin", "main"]).unwrap();
    git(&repo, &["checkout", "-q", "-b", "lane"]).unwrap();
    fs::write(repo.join("crates/a/src/x.rs"), "one\ntwo\nthree\nfour\n").unwrap();
    let head = commit(&repo, "Lane edits x");
    git(&repo, &["checkout", "-q", "main"]).unwrap();
    let lander = Lander { repo: repo.clone(), worktree: candidate::lander_worktree(&repo, "lander").unwrap(), queue: dir.path().join("queue"), checker: Vec::new(), app_token: Vec::new(), offload_owner: String::new(), round: AtomicU64::new(0), chores: Chores::default(), heartbeat: Heartbeat::default(), wake: std::sync::Arc::default() };
    fs::create_dir_all(&lander.queue).unwrap();
    let ticket = Ticket { branch: "lane".into(), head, root: repo.clone(), origin: origin.display().to_string(), ..Ticket::default() };
    queue::write(&lander.queue.join("1-lane.json"), &ticket).unwrap();
    let serve = Serve::new(&lander);
    assert!(!serve.sweep_moved(None), "the first sweep has no earlier main to compare");

    fs::create_dir_all(repo.join("crates/b/src")).unwrap();
    git(&repo, &["rm", "-q", "crates/a/src/x.rs"]).unwrap();
    fs::write(repo.join("crates/b/src/x.rs"), "wholly\nnew\nmodule\nbody\n").unwrap();
    let split = commit(&repo, "Split crate a into a and b");
    git(&repo, &["push", "-q", "origin", "main"]).unwrap();

    assert!(serve.sweep_moved(None), "the lane on the moved file was not sent back in the round after the split landed");
    assert!(queue::collect(&lander.queue).unwrap().is_empty(), "the sent-back lane is still queued");
    let sent = fs::read_to_string(lander.queue.join("done/1-lane.json")).unwrap();
    let short = git(&repo, &["rev-parse", "--short", &split]).unwrap();
    let verdict = format!("conflict crates/a/src/x.rs after landing {short} Split crate a into a and b");
    assert!(sent.contains(&verdict), "the send-back does not name the splitting landing {split}: {sent}");
}
