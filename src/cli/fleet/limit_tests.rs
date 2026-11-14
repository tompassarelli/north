use std::process::Command;

use super::*;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").arg("-C").arg(dir).args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn ticket(session: &str, branch: &str, verdict: Option<&str>) -> String {
    serde_json::json!({"origin": "origin", "root": "/lane", "host": "host", "branch": branch, "head": "head", "session": session, "queued_at": "2026-11-14T00:00:00Z", "verdict": verdict}).to_string()
}

/// On 2026-11-07 chr-north-1107-B, launched in the primary checkout whose HEAD held the unpushed north#791 commit, was
/// handed over for that commit as if it were its own (north#801): a primary checkout's HEAD is no session's lane, while
/// the same commit in a linked worktree it runs in, or a land ticket still in the queue, is. On 2026-11-14 the lander's
/// verdict told the two unqueued cases apart (north#259): a lane the lander sent back, whose ticket is decided under
/// `done/`, and a lane never enqueued; a lane whose ticket still waits in the queue is neither.
#[test]
fn an_unqueued_lane_is_told_apart_as_sent_back_or_never_enqueued_north_259() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main, lane, state) = (root.path().join("origin.git"), root.path().join("main"), root.path().join("lane"), root.path().join("state"));
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

    let queue_dir = queue::directory(&state, origin.to_str().unwrap());
    std::fs::create_dir_all(queue_dir.join("done")).unwrap();
    std::fs::write(queue_dir.join("done/1794000000000000-lane.json"), ticket("chr-north-1107-B", "lane", Some("conflict: placement.rs moved"))).unwrap();
    let tickets = crate::land_ticket::lanes_by_session(&crate::platform::land_queue::queues(&state)).unwrap();
    assert!(holds_unlanded_lane("chr-north-1107-B", &main, &tickets));
    assert_eq!(unqueued_lane("chr-north-1107-B", &lane, &state, &tickets), Some(Unqueued::SentBack { branch: "lane".to_owned(), cause: None }));
    assert_eq!(unqueued_lane("root-from-github-91", &lane, &state, &tickets), Some(Unqueued::NeverEnqueued));
    assert_eq!(unqueued_lane("chr-north-1107-B", &main, &state, &tickets), None);

    std::fs::write(queue_dir.join("1794000000000001-lane.json"), ticket("root-from-github-91", "lane", None)).unwrap();
    let tickets = crate::land_ticket::lanes_by_session(&crate::platform::land_queue::queues(&state)).unwrap();
    assert_eq!(unqueued_lane("root-from-github-91", &lane, &state, &tickets), None);
}

/// On 2026-11-09 root-from-github-91 ran in the primary checkout south/main and worked in a worktree it made under
/// south/worktrees with its commit ready and no land ticket: neither test saw its work, so the tick would have moved it
/// nowhere (north#1007). The worktree its output names is its lane; a primary checkout whose output names none stays itself.
#[test]
fn a_worker_in_a_primary_checkout_is_handed_over_for_the_worktree_it_made_beside_it_north_1007() {
    let root = tempfile::tempdir().unwrap();
    let (origin, main) = (root.path().join("origin.git"), root.path().join("main"));
    let lane = root.path().join("worktrees/ready");
    git(root.path(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    git(root.path(), &["clone", "-q", origin.to_str().unwrap(), main.to_str().unwrap()]);
    git(&main, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&main, &["push", "-q", "origin", "HEAD:main"]);
    git(&main, &["worktree", "add", "-q", "-b", "ready", lane.to_str().unwrap()]);
    std::fs::write(lane.join("fix.rs"), "fix").unwrap();
    git(&lane, &["add", "fix.rs"]);
    git(&lane, &["commit", "-q", "-m", "ready"]);
    let none = BTreeMap::new();
    let made = lane_of(&main, |worktrees| Some(worktrees.join("ready")));
    assert_eq!(made, lane);
    assert!(holds_unlanded_lane("root-from-github-91", &made, &none));
    assert_eq!(lane_of(&main, |_| None), main);
    assert_eq!(lane_of(&lane, |_| panic!("a linked worktree is read as it is")), lane);
}
