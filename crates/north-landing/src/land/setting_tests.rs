use std::path::Path;
use std::process::Command;

use super::landing;

fn git(repo: &Path, arguments: &[&str]) {
    let ran = Command::new("git").arg("-C").arg(repo).args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"]).args(arguments).output().unwrap();
    assert!(ran.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&ran.stderr));
}

#[test]
fn an_eval_clone_whose_safe_push_sets_no_remote_hosts_serves_none_of_the_machines_remote_hosts() {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path();
    git(repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join(".safe-push"), "[landing]\n\tqueue = lander\n\tlander-host = cpx62\n\tremote-hosts =\n").unwrap();
    git(repo, &["add", ".safe-push"]);
    git(repo, &["commit", "-qm", "Land this eval clone through its scratch lander"]);
    git(repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(repo, &["config", "landing.remote-hosts", "gex45 nexus"]);
    git(repo, &["config", "landing.capacity", "north capacity host run --"]);

    assert_eq!(landing(repo, "refs/remotes/origin/main", "landing.remote-hosts"), "", "L5's scratch lander on cpx62 dialed the machine's gex45 and nexus outboxes about 1,280 times each");
    assert_eq!(landing(repo, "refs/remotes/origin/main", "landing.lander-host"), "cpx62");
    assert_eq!(landing(repo, "refs/remotes/origin/main", "landing.capacity"), "north capacity host run --", "a key the committed file leaves out still comes from the machine");
}
