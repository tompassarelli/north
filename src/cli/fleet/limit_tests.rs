use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// On 2026-11-07 chr-north-1107-B, launched in the primary checkout whose HEAD held the unpushed north#791 commit, was
/// handed over for that commit as if it were its own (north#801): a primary checkout's HEAD is no session's lane, while
/// the same commit in a linked worktree it runs in, or a land ticket still in the queue, is.
#[test]
fn a_worker_in_a_primary_checkout_holding_a_foreign_unpushed_commit_is_not_handed_over_for_it_north_801() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main, lane) = (root.path().join("origin.git"), root.path().join("main"), root.path().join("lane"));
    git(root.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(root.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(&main, &["fetch", "-q", "origin"]);
    std::fs::write(main.join("placement.rs"), "class").unwrap();
    git(&main, &["add", "placement.rs"]);
    git(&main, &["commit", "-q", "-m", "north#791 placement class"]);
    git(&main, &["worktree", "add", "-q", "-b", "lane", lane.to_str().unwrap()]);
    let none = BTreeMap::new();
    assert!(!holds_unlanded_lane("chr-north-1107-B", &main, &none));
    assert!(holds_unlanded_lane("chr-north-1107-B", &lane, &none));
    let queues = root.path().join("land-queue");
    std::fs::create_dir_all(queues.join("north")).unwrap();
    std::fs::write(queues.join("north/1794000000000000-lane.json"), r#"{"session":"chr-north-1107-B","branch":"lane"}"#).unwrap();
    let tickets = crate::land_ticket::lanes_by_session(&queues).unwrap();
    assert!(tickets.contains_key("chr-north-1107-B"), "{tickets:?}");
    assert!(holds_unlanded_lane("chr-north-1107-B", &main, &tickets));
}
